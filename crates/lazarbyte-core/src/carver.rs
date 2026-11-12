//! Scan engine: stream the source, match signatures, carve hits to disk.
//!
//! The scan is a chunked sweep with an overlap equal to the longest magic, so
//! a header straddling a chunk boundary is still found. Candidate offsets come
//! from one Aho-Corasick pass (SIMD prefilter) over each chunk.

use crate::handlers::Carve;
use crate::reader::Source;
use crate::signatures::Signature;
use crate::window::Window;
use aho_corasick::{AhoCorasick, MatchKind};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;

/// How much room a scan has left on its output volume.
///
/// Implemented by the caller, because the answer differs by platform: a desktop
/// run asks the filesystem about a directory, while the Android bridge asks
/// about the file descriptor it was given. The core only needs the number.
pub trait FreeSpace: Send + Sync {
    /// Bytes available to an unprivileged writer, or `None` if unknown.
    /// Unknown never stops a scan: a missing answer must not look like a full
    /// disk.
    fn free_bytes(&self) -> Option<u64>;
}

/// Re-probe the volume at most once per this many written bytes. A statvfs per
/// carved file would be one syscall per file; one per 16 MiB is invisible.
pub const FREE_SPACE_CHECK_BYTES: u64 = 16 << 20;

const STOP_NONE: u8 = 0;
const STOP_LIMIT: u8 = 1;
const STOP_FREE: u8 = 2;
const STOP_HALTED: u8 = 3;

#[derive(Clone, Debug)]
pub struct Record {
    pub kind: &'static str,
    pub ext: &'static str,
    pub offset: u64,
    pub size: u64,
    pub sha256: String,
    pub validated: bool,
    pub path: String,
    pub duplicate_of: Option<u64>,
    /// What deep validation concluded, when it ran: `Some(true)` decoded,
    /// `Some(false)` failed to decode, `None` not attempted or inconclusive.
    pub decoded: Option<bool>,
}

impl Record {
    pub fn confidence(&self) -> &'static str {
        match self.decoded {
            // A decode is stronger evidence than a structure walk, in both
            // directions: it can confirm a file the walk was unsure of, and
            // condemn one the walk accepted.
            Some(true) => "verified",
            Some(false) => "failed",
            None if self.validated => "high",
            None => "low",
        }
    }
}

/// What a running scan has got through, for reporting while it runs.
///
/// A scan of a real disk takes hours, and until this existed the only output
/// was the summary at the end: an analyst had no way to tell a slow scan from a
/// stuck one, or to know whether to wait ten minutes or ten hours.
#[derive(Default)]
pub struct Progress {
    scanned: AtomicU64,
    files: AtomicU64,
    bytes_out: AtomicU64,
    /// Total this scan intends to read, for the percentage.
    pub total: u64,
}

impl Progress {
    pub fn new(total: u64) -> Self {
        Progress {
            total,
            ..Default::default()
        }
    }

    pub fn add_scanned(&self, n: u64) {
        self.scanned.fetch_add(n, Ordering::Relaxed);
    }

    pub fn add_file(&self, bytes: u64) {
        self.files.fetch_add(1, Ordering::Relaxed);
        self.bytes_out.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn scanned(&self) -> u64 {
        self.scanned.load(Ordering::Relaxed)
    }

    pub fn files(&self) -> u64 {
        self.files.load(Ordering::Relaxed)
    }

    pub fn bytes_out(&self) -> u64 {
        self.bytes_out.load(Ordering::Relaxed)
    }
}

/// Shared limit on what a scan may write.
///
/// A carve of a large disk can produce more than the volume it is written to
/// can hold -- on a 238 GB image an unfiltered run reached 51 GB inside the
/// first percent and filled the filesystem, which takes the machine down with
/// it. Workers charge every byte here and stop cleanly when the budget or the
/// free-space floor is reached, so the manifest still lands.
#[derive(Default)]
pub struct OutputBudget {
    written: AtomicU64,
    limit: u64,
    /// Stop once the output volume has this little room left (0 = no floor).
    min_free: u64,
    free: Option<Arc<dyn FreeSpace>>,
    /// Written-bytes watermark for the next free-space probe.
    next_free_check: AtomicU64,
    stop: AtomicBool,
    stop_reason: AtomicU8,
}

impl OutputBudget {
    pub fn new(limit: u64) -> Self {
        OutputBudget {
            limit,
            ..Default::default()
        }
    }

    /// Build the budget a scan described by `opts` should use.
    pub fn for_options(opts: &Options) -> Self {
        OutputBudget {
            min_free: opts.min_free,
            free: opts.free_space.clone(),
            ..OutputBudget::new(opts.max_output)
        }
    }

    /// Enforce a free-space floor while writing. `free` is asked at most once
    /// per `FREE_SPACE_CHECK_BYTES`, so this costs nothing on a fast scan.
    pub fn with_free_space(mut self, min_free: u64, free: Arc<dyn FreeSpace>) -> Self {
        self.min_free = min_free;
        self.free = Some(free);
        self
    }

    pub fn charge(&self, bytes: u64) {
        self.written.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    fn halt_with(&self, reason: u8) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.stop_reason.compare_exchange(
            STOP_NONE,
            reason,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }

    /// True when the scan stopped because the output volume ran low.
    pub fn stopped_by_free_space(&self) -> bool {
        self.stop_reason.load(Ordering::Relaxed) == STOP_FREE
    }

    /// True when the scan stopped at the `max_output` ceiling.
    pub fn stopped_by_limit(&self) -> bool {
        self.stop_reason.load(Ordering::Relaxed) == STOP_LIMIT
    }

    /// Has the scan hit its limit? Sticky, so every worker sees it.
    pub fn exhausted(&self) -> bool {
        if self.stop.load(Ordering::Relaxed) {
            return true;
        }
        if self.limit > 0 && self.written() >= self.limit {
            self.halt_with(STOP_LIMIT);
            return true;
        }
        if self.min_free > 0 {
            if let Some(free) = &self.free {
                let written = self.written();
                if written >= self.next_free_check.load(Ordering::Relaxed) {
                    // Advance first: a second worker may probe too, which is
                    // harmless, but nobody should probe on every call.
                    self.next_free_check.store(
                        written.saturating_add(FREE_SPACE_CHECK_BYTES),
                        Ordering::Relaxed,
                    );
                    if free.free_bytes().is_some_and(|n| n <= self.min_free) {
                        self.halt_with(STOP_FREE);
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn halt(&self) {
        self.halt_with(STOP_HALTED);
    }
}

#[derive(Clone)]
pub struct Options {
    pub out_dir: String,
    pub chunk_size: u64,
    pub align: u64,
    pub skip_carved: bool,
    pub min_size: u64,
    pub max_size: u64,
    pub start: u64,
    pub length: u64,
    pub window_end: u64,
    pub dry_run: bool,
    pub quiet: bool,
    pub dedup: bool,
    pub skip_blank: bool,
    pub jobs: usize,
    /// Stop writing after this many bytes of carved output (0 = no limit).
    pub max_output: u64,
    /// Stop when the output filesystem has less than this much free (0 = off).
    pub min_free: u64,
    /// Where `min_free` is measured. The caller supplies it: a directory on
    /// desktop, a file descriptor on Android. `None` means the floor cannot
    /// be enforced, so the scan runs unchecked.
    pub free_space: Option<Arc<dyn FreeSpace>>,
    /// Decode carved bytes to confirm the file is intact, not just well formed.
    pub validate: bool,
    /// Do not keep a carve whose decode failed.
    pub drop_failed: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            out_dir: "carved".into(),
            chunk_size: 32 << 20,
            align: 1,
            skip_carved: true,
            min_size: 0,
            max_size: 0,
            start: 0,
            length: 0,
            window_end: 0,
            dry_run: false,
            quiet: false,
            dedup: true,
            skip_blank: true,
            jobs: 1,
            max_output: 0,
            min_free: 0,
            free_space: None,
            validate: false,
            drop_failed: false,
        }
    }
}

pub struct Carver<'a> {
    reader: &'a Source,
    progress: Option<&'a Progress>,
    opts: &'a Options,
    /// Budget shared with the other workers of a parallel scan.
    budget: Option<&'a OutputBudget>,
    /// Budget for a carver used on its own, so the limit holds however the
    /// scan engine is entered.
    owned_budget: Option<OutputBudget>,
    matcher: AhoCorasick,
    /// pattern index -> signatures owning that magic
    by_pattern: Vec<Vec<&'static Signature>>,
    pattern_len: Vec<usize>,
    pub rejected: u64,
    pub skipped_blank: u64,
    window_end: u64,
}

impl<'a> Carver<'a> {
    pub fn new(reader: &'a Source, sigs: Vec<&'static Signature>, opts: &'a Options) -> Self {
        // One pattern per distinct magic; longest match wins so the most
        // specific magic is preferred, matching the Python alternation order.
        let mut patterns: Vec<Vec<u8>> = Vec::new();
        let mut by_pattern: Vec<Vec<&'static Signature>> = Vec::new();
        for sig in &sigs {
            for magic in sig.magics {
                let m = magic.to_vec();
                match patterns.iter().position(|p| *p == m) {
                    Some(i) => by_pattern[i].push(sig),
                    None => {
                        patterns.push(m);
                        by_pattern.push(vec![sig]);
                    }
                }
            }
        }
        let pattern_len = patterns.iter().map(|p| p.len()).collect();
        let matcher = AhoCorasick::builder()
            .match_kind(MatchKind::LeftmostLongest)
            .build(&patterns)
            .expect("signature magics build an automaton");
        Carver {
            reader,
            progress: None,
            opts,
            budget: None,
            owned_budget: (opts.max_output > 0 || (opts.min_free > 0 && opts.free_space.is_some()))
                .then(|| OutputBudget::for_options(opts)),
            matcher,
            by_pattern,
            pattern_len,
            rejected: 0,
            skipped_blank: 0,
            window_end: 0,
        }
    }

    /// Report progress into a counter shared with the other workers.
    pub fn with_progress(mut self, progress: &'a Progress) -> Self {
        self.progress = Some(progress);
        self
    }

    /// Share an output budget with the other workers of this scan.
    pub fn with_budget(mut self, budget: &'a OutputBudget) -> Self {
        self.budget = Some(budget);
        self
    }

    /// True once the scan should stop writing.
    pub fn stopped(&self) -> bool {
        if let Some(b) = self.budget {
            return b.exhausted();
        }
        self.owned_budget
            .as_ref()
            .map(|b| b.exhausted())
            .unwrap_or(false)
    }

    pub fn run(&mut self) -> Vec<Record> {
        let o = self.opts;
        let mut records: Vec<Record> = Vec::new();
        let scan_end = if o.length > 0 {
            (o.start + o.length).min(self.reader.size())
        } else {
            self.reader.size()
        };
        self.window_end = if o.window_end > 0 {
            o.window_end
        } else {
            scan_end
        };
        let overlap = self.pattern_len.iter().copied().max().unwrap_or(1) as u64 - 1 + 4;
        let mut pos = o.start;
        let mut next_allowed = o.start;

        while pos < scan_end {
            if self.stopped() {
                break;
            }
            let want = (o.chunk_size + overlap).min(scan_end - pos + overlap);
            let buf = self.reader.pread(pos, want as usize);
            if buf.is_empty() {
                break;
            }
            let limit = (buf.len() as u64).min(o.chunk_size);
            // Blank-block skip: an all-zero chunk (TRIM'd/sparse) holds no headers.
            if o.skip_blank && buf[..limit as usize].iter().all(|&b| b == 0) {
                self.skipped_blank += limit;
                pos += limit;
                if let Some(p) = self.progress {
                    p.add_scanned(limit);
                }
                continue;
            }
            // Collect the chunk's candidate offsets before carving: carving
            // needs &mut self, and the match iterator borrows the automaton.
            let hits: Vec<(u64, usize)> = self
                .matcher
                .find_iter(&buf)
                .map(|m| (m.start() as u64, m.pattern().as_usize()))
                .filter(|&(i, _)| i < limit)
                .collect();
            for (i, pat) in hits {
                // Per candidate, not per chunk: a single 32 MiB chunk can hold
                // thousands of files, so a chunk-level check overshoots wildly.
                if self.stopped() {
                    break;
                }
                let abs_magic = pos + i;
                for sig in self.by_pattern[pat].clone() {
                    if abs_magic < sig.header_offset {
                        continue;
                    }
                    let start = abs_magic - sig.header_offset;
                    if start < o.start || abs_magic >= scan_end {
                        continue;
                    }
                    if start < next_allowed && o.skip_carved {
                        continue;
                    }
                    if o.align > 1 && start % o.align != 0 {
                        continue;
                    }
                    if let Some(pre) = sig.precheck {
                        if !pre(&buf, i as usize) {
                            continue;
                        }
                    }
                    if let Some(rec) = self.try_carve(sig, start) {
                        let validated = rec.validated;
                        let end = rec.offset + rec.size;
                        if let Some(p) = self.progress {
                            p.add_file(rec.size);
                        }
                        records.push(rec);
                        if o.skip_carved && validated {
                            next_allowed = end;
                        }
                        break;
                    }
                }
            }
            pos += limit;
            if let Some(p) = self.progress {
                p.add_scanned(limit);
            }
        }
        if o.dedup {
            dedupe(&mut records, o.dry_run);
        }
        records
    }

    fn try_carve(&mut self, sig: &'static Signature, start: u64) -> Option<Record> {
        let o = self.opts;
        let mut cap = sig.max_size;
        if o.max_size > 0 {
            cap = cap.min(o.max_size);
        }
        cap = cap.min(self.window_end.saturating_sub(start));
        if cap == 0 {
            return None;
        }
        let carve: Carve = {
            let mut w = Window::new(self.reader, start, cap);
            match sig.carve(&mut w) {
                Some(c) => c,
                None => {
                    self.rejected += 1;
                    return None;
                }
            }
        };
        if carve.size < o.min_size.max(1) {
            self.rejected += 1;
            return None;
        }
        // A handler must not report more than its window holds -- the bytes
        // past the end are not the file. Enforced here as well as in each
        // handler, so one arithmetic slip in one of 28 parsers cannot write
        // unrelated disk into evidence.
        if carve.size > cap {
            self.rejected += 1;
            return None;
        }

        // Deep validation needs the whole file at once, so a carve small
        // enough to hold in memory is read, decoded and only then written --
        // that way a tightened length is what lands on disk, and a failed
        // decode need never be written at all.
        let mut decoded: Option<bool> = None;
        let mut buffered: Option<Vec<u8>> = None;
        let mut carve_size = carve.size;
        let mut validated = carve.validated;
        if o.validate
            && carve.size <= crate::validate::MAX_VALIDATE
            && crate::validate::can_validate(carve.ext)
        {
            let mut data = Vec::with_capacity(carve.size as usize);
            let mut got = 0u64;
            while got < carve.size {
                let want = ((carve.size - got) as usize).min(8 << 20);
                let blk = self.reader.pread(start + got, want);
                if blk.is_empty() {
                    break;
                }
                got += blk.len() as u64;
                data.extend_from_slice(&blk);
            }
            match crate::validate::validate(carve.ext, &data) {
                crate::validate::Verdict::Verified(tighter) => {
                    decoded = Some(true);
                    validated = true;
                    if let Some(n) = tighter {
                        if n > 0 && n <= data.len() as u64 {
                            data.truncate(n as usize);
                            carve_size = n;
                        }
                    }
                }
                crate::validate::Verdict::Invalid => {
                    decoded = Some(false);
                    validated = false;
                    if o.drop_failed {
                        self.rejected += 1;
                        return None;
                    }
                }
                crate::validate::Verdict::Inconclusive => {}
            }
            buffered = Some(data);
        }

        // Hash while streaming; write as we go unless this is a dry run.
        let mut hasher = Sha256::new();
        let mut path = String::new();
        let mut file = if o.dry_run {
            None
        } else {
            let dir = PathBuf::from(&o.out_dir).join(carve.ext);
            if fs::create_dir_all(&dir).is_err() {
                return None;
            }
            let p = dir.join(format!("f_{:012x}.{}", start, carve.ext));
            path = p.to_string_lossy().to_string();
            match fs::File::create(&p) {
                Ok(f) => Some(std::io::BufWriter::new(f)),
                Err(_) => return None,
            }
        };
        let mut done: u64 = 0;
        if let Some(data) = buffered {
            hasher.update(&data);
            if let Some(f) = file.as_mut() {
                if f.write_all(&data).is_err() {
                    return None;
                }
            }
            done = data.len() as u64;
        }
        while done < carve_size {
            let want = ((carve_size - done) as usize).min(8 << 20);
            let blk = self.reader.pread(start + done, want);
            if blk.is_empty() {
                break;
            }
            hasher.update(&blk);
            if let Some(f) = file.as_mut() {
                if f.write_all(&blk).is_err() {
                    return None;
                }
            }
            done += blk.len() as u64;
        }
        if let Some(mut f) = file {
            let _ = f.flush();
        }
        if !o.dry_run {
            if let Some(budget) = self.budget {
                budget.charge(done);
            } else if let Some(budget) = &self.owned_budget {
                budget.charge(done);
            }
        }

        Some(Record {
            kind: sig.name,
            ext: carve.ext,
            offset: start,
            size: done,
            sha256: format!("{:x}", hasher.finalize()),
            validated,
            path,
            duplicate_of: None,
            decoded,
        })
    }
}

/// Drop carves that fall entirely inside an earlier validated carve.
///
/// A serial scan skips ahead past a validated carve, so it never looks inside
/// one; parallel ranges cannot see each other's skip-ahead state, and a worker
/// starting mid-archive would otherwise report an inner member (a file stored
/// in a docx, say) as a separate carve. Without this, `-j` and a serial run
/// disagree on the same image.
pub fn containment_filter(records: Vec<Record>) -> Vec<Record> {
    let mut sorted = records;
    sorted.sort_by_key(|r| (r.offset, std::cmp::Reverse(r.size)));
    let mut out: Vec<Record> = Vec::with_capacity(sorted.len());
    let mut covered_end: u64 = 0;
    for rec in sorted {
        if covered_end > 0 && rec.offset + rec.size <= covered_end {
            if !rec.path.is_empty() {
                let _ = fs::remove_file(&rec.path);
            }
            continue;
        }
        if rec.validated {
            covered_end = covered_end.max(rec.offset + rec.size);
        }
        out.push(rec);
    }
    out
}

/// Mark byte-identical carves as duplicates of the first one seen, dropping
/// the redundant copies from disk.
pub fn dedupe(records: &mut [Record], dry_run: bool) {
    let mut first: HashMap<String, u64> = HashMap::new();
    for rec in records.iter_mut() {
        match first.get(&rec.sha256) {
            None => {
                first.insert(rec.sha256.clone(), rec.offset);
            }
            Some(&origin) => {
                rec.duplicate_of = Some(origin);
                if !dry_run && !rec.path.is_empty() {
                    let _ = fs::remove_file(&rec.path);
                    rec.path = String::new();
                }
            }
        }
    }
}

/// Split [start, end) into `jobs` ranges and scan them on separate threads.
///
/// Carve windows may run past a range's end (`window_end`), so a file whose
/// header sits near a boundary is still carved whole by the worker that owns
/// the header -- the same contract as the Python `run_parallel`.
/// Scan the ranges given, checkpointing each as it completes.
///
/// Used by --resume: the caller works out which parts are still outstanding and
/// hands them over, so a run that died leaves the rest of the work intact.
#[allow(clippy::too_many_arguments)]
pub fn run_ranges(
    reader: &Source,
    sigs: &[&'static Signature],
    opts: &Options,
    ranges: &[(u64, u64)],
    scan_end: u64,
    progress: Option<&Progress>,
    on_range_done: impl FnMut(u64, u64, &[Record]) + Send,
) -> Vec<Record> {
    let budget = OutputBudget::for_options(opts);
    run_ranges_with_budget(
        reader,
        sigs,
        opts,
        &budget,
        ranges,
        scan_end,
        progress,
        on_range_done,
    )
}

/// `run_ranges` with a caller-owned budget, so the caller can tell afterwards
/// whether the scan finished or stopped on the output limit / free-space floor.
#[allow(clippy::too_many_arguments)]
pub fn run_ranges_with_budget(
    reader: &Source,
    sigs: &[&'static Signature],
    opts: &Options,
    budget: &OutputBudget,
    ranges: &[(u64, u64)],
    scan_end: u64,
    progress: Option<&Progress>,
    on_range_done: impl FnMut(u64, u64, &[Record]) + Send,
) -> Vec<Record> {
    // Ranges finish at different times, and one that finished should be
    // recorded then rather than when the slowest of its batch does: a live scan
    // of a 237 GB image had recorded nothing after sixteen hours because a whole
    // batch had to join first, so a killed run would have restarted from zero.
    let on_range_done = std::sync::Mutex::new(on_range_done);
    let collected: std::sync::Mutex<Vec<Record>> = std::sync::Mutex::new(Vec::new());
    let jobs = opts.jobs.max(1);

    // A queue rather than fixed batches. Free-space ranges vary from a few KiB
    // to tens of gigabytes, and with batches every worker waited for the
    // slowest in its group -- on a fragmented volume that is most of the time
    // spent idle. Long-lived workers also build the pattern automaton once
    // instead of once per range, which matters when there are 46,774 of them.
    let next = AtomicU64::new(0);
    std::thread::scope(|scope| {
        for _ in 0..jobs {
            let next = &next;
            let done = &on_range_done;
            let collected = &collected;
            let sigs = sigs.to_vec();
            scope.spawn(move || {
                let mut mine: Vec<Record> = Vec::new();
                loop {
                    if budget.exhausted() {
                        break;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed) as usize;
                    let Some(&(start, end)) = ranges.get(i) else {
                        break;
                    };
                    let mut sub = opts.clone();
                    sub.start = start;
                    sub.length = end - start;
                    sub.window_end = scan_end; // carve past a range end when needed
                    sub.quiet = true;
                    sub.dedup = false; // one dedup pass over the merged result
                    sub.jobs = 1;
                    let mut c = Carver::new(reader, sigs.clone(), &sub).with_budget(budget);
                    if let Some(p) = progress {
                        c = c.with_progress(p);
                    }
                    let recs = c.run();
                    if !budget.exhausted() {
                        // The records go out with the range that produced them,
                        // so a run that is killed still leaves a record of what
                        // it found: a manifest written only at the end means a
                        // killed scan leaves files nobody can account for.
                        if let Ok(mut f) = done.lock() {
                            f(start, end, &recs);
                        }
                    }
                    mine.extend(recs);
                }
                if let Ok(mut all) = collected.lock() {
                    all.extend(mine);
                }
            });
        }
    });

    let out = collected.into_inner().unwrap_or_default();
    let mut out = containment_filter(out);
    if opts.dedup {
        dedupe(&mut out, opts.dry_run);
    }
    out
}

pub fn run_parallel(reader: &Source, sigs: &[&'static Signature], opts: &Options) -> Vec<Record> {
    let budget = OutputBudget::for_options(opts);
    run_parallel_with_budget(reader, sigs, opts, &budget)
}

/// `run_parallel` with a caller-owned budget, so the caller can tell afterwards
/// whether the scan finished or stopped early.
pub fn run_parallel_with_budget(
    reader: &Source,
    sigs: &[&'static Signature],
    opts: &Options,
    budget: &OutputBudget,
) -> Vec<Record> {
    let scan_end = if opts.length > 0 {
        (opts.start + opts.length).min(reader.size())
    } else {
        reader.size()
    };
    let total = scan_end.saturating_sub(opts.start);
    let jobs = opts.jobs.max(1);
    if jobs == 1 || total == 0 {
        let mut c = Carver::new(reader, sigs.to_vec(), opts).with_budget(budget);
        let out = c.run();
        let mut out = containment_filter(out);
        if opts.dedup {
            dedupe(&mut out, opts.dry_run);
        }
        return out;
    }
    let span = total / jobs as u64 + 1;
    let mut out: Vec<Record> = Vec::new();
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for j in 0..jobs {
            let range_start = opts.start + span * j as u64;
            if range_start >= scan_end {
                break;
            }
            let range_len = span.min(scan_end - range_start);
            let mut sub = opts.clone();
            sub.start = range_start;
            sub.length = range_len;
            sub.window_end = scan_end; // carve past the range end when needed
            sub.quiet = true;
            sub.dedup = false; // one dedup pass over the merged result instead
            sub.jobs = 1;
            let sigs = sigs.to_vec();
            handles.push(scope.spawn(move || {
                let mut c = Carver::new(reader, sigs, &sub).with_budget(budget);
                c.run()
            }));
        }
        for h in handles {
            out.extend(h.join().expect("scan worker panicked"));
        }
    });
    let mut out = containment_filter(out);
    if opts.dedup {
        dedupe(&mut out, opts.dry_run);
    }
    out
}
