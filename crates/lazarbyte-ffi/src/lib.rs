//! uniffi bindings over `lazarbyte-core`.
//!
//! The engines live in `lazarbyte-core`; this crate only adapts them to the
//! types uniffi can cross the FFI boundary with. Business logic stays out: an
//! exported function parses arguments, calls one core entry point and maps its
//! `Result` to an error the foreign side can catch.
//!
//! The whole crate is unsafe-free, with one approved exception: `fdsource.rs`,
//! which owns a file descriptor handed over by Kotlin and is the single
//! module allowed to contain unsafe code (AGENTS.md §3, §10). The crate-level
//! lint is `deny` rather than `forbid` only so that this one module can lift
//! it; any unsafe outside that module still fails the build.
#![deny(unsafe_code)]

use std::sync::Arc;
use std::time::Duration;

use lazarbyte_core::carver::{self, OutputBudget};
use lazarbyte_core::reader::Source;
use lazarbyte_core::signatures::{resolve_types, Signature, SIGNATURES};

mod fdsource;

// Proc-macro-only API: no UDL file, no build.rs (the current official
// recommendation, uniffi 0.32).
uniffi::setup_scaffolding!();

/// Proof the bridge links, and the version the app can show.
#[uniffi::export]
pub fn bridge_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Why a scan refused or failed. The core's errors are strings; the app shows
/// the text and decides what to do next.
#[derive(Debug, uniffi::Error)]
#[uniffi(flat_error)]
pub enum ScanError {
    /// The message is the core's own explanation, ready to display.
    Failed(String),
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScanError::Failed(msg) => f.write_str(msg),
        }
    }
}

fn scan_error(msg: impl Into<String>) -> ScanError {
    ScanError::Failed(msg.into())
}

/// An open image the scan engines read from.
///
/// Sources are read-only by construction: whatever was opened, no code path
/// in the core holds a writable handle to it (AGENTS.md §3).
#[derive(uniffi::Object)]
pub struct SourceHandle {
    inner: Source,
}

#[uniffi::export]
impl SourceHandle {
    /// Open an image by path: the desktop way, and the emulator smoke test's.
    #[uniffi::constructor]
    pub fn from_path(path: String) -> Result<Self, ScanError> {
        Ok(SourceHandle {
            inner: Source::open(&path).map_err(|e| scan_error(e.to_string()))?,
        })
    }

    /// Open an image from a file descriptor handed over by the app.
    ///
    /// The descriptor must have been detached with
    /// `ParcelFileDescriptor.detachFd()`: ownership crosses the boundary here,
    /// and the bridge closes it when the handle is dropped. See `fdsource.rs`
    /// for the safety contract.
    #[uniffi::constructor]
    pub fn from_fd(fd: i32) -> Result<Self, ScanError> {
        #[cfg(unix)]
        {
            Ok(SourceHandle {
                inner: fdsource::source_from_fd(fd).map_err(|e| scan_error(e.to_string()))?,
            })
        }
        #[cfg(not(unix))]
        {
            let _ = fd;
            Err(scan_error(
                "opening a source from a descriptor is only available on unix",
            ))
        }
    }

    /// How many bytes the image holds, for the pre-scan estimate.
    pub fn size(&self) -> u64 {
        self.inner.size()
    }
}

/// Ask a running scan to stop.
///
/// [`scan_carve`] checks this between chunks and between files, so a cancelled
/// scan stops within a bounded amount of work; whatever was already carved
/// stays on disk and the run reports the records it had written. The flag is
/// sticky and safe to set from any thread, any number of times.
#[derive(uniffi::Object)]
pub struct ScanCancel {
    inner: Arc<lazarbyte_core::cancel::Cancel>,
}

impl Default for ScanCancel {
    fn default() -> Self {
        ScanCancel {
            inner: Arc::new(lazarbyte_core::cancel::Cancel::new()),
        }
    }
}

#[uniffi::export]
impl ScanCancel {
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the scan to stop. Idempotent.
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    /// Has the scan been asked to stop? Never resets.
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
}

/// How far a running scan has got, reported while it runs.
///
/// Called from a background thread the bridge owns; the foreign implementation
/// must be safe to call from any thread.
#[uniffi::export(callback_interface)]
pub trait ProgressCallback: Send + Sync {
    fn on_progress(&self, scanned: u64, total: u64, files: u64, bytes_out: u64);
}

/// One carved file, the same record the CLI's manifest carries.
#[derive(Debug, uniffi::Record)]
pub struct CarveRecord {
    pub kind: String,
    pub ext: String,
    pub offset: u64,
    pub size: u64,
    pub sha256: String,
    pub validated: bool,
    pub path: String,
    pub duplicate_of: Option<u64>,
    /// What decode validation concluded: `true` decoded, `false` failed,
    /// `None` not attempted or inconclusive.
    pub decoded: Option<bool>,
    /// The core's confidence wording: verified / failed / high / low.
    pub confidence: String,
}

/// The carve scan's settings. Defaults match the desktop CLI; the mobile
/// profile is [`CarveOptions::mobile`].
#[derive(uniffi::Record)]
pub struct CarveOptions {
    /// Where carved files go. A SAF-backed sink arrives with the Android
    /// bridge; until then this must be a writable directory path.
    pub out_dir: String,
    /// Signature type names or group names ("image", "archives"); empty means
    /// every signature the build carries.
    pub signature_types: Vec<String>,
    pub start: u64,
    /// Scan only this many bytes; `0` means to the end of the image.
    pub length: u64,
    pub min_size: u64,
    /// Carves larger than this are rejected; `0` means no ceiling.
    pub max_size: u64,
    pub chunk_size: u64,
    pub align: u64,
    pub dry_run: bool,
    pub dedup: bool,
    pub skip_blank: bool,
    pub skip_carved: bool,
    pub jobs: u32,
    /// Stop writing after this many bytes of carved output; `0` = no limit.
    pub max_output: u64,
    pub validate: bool,
    pub drop_failed: bool,
    /// Largest carve buffered for decode validation: a memory ceiling.
    pub max_validate: u64,
}

impl CarveOptions {
    fn to_core(&self) -> carver::Options {
        carver::Options {
            out_dir: self.out_dir.clone(),
            start: self.start,
            length: self.length,
            min_size: self.min_size,
            max_size: self.max_size,
            chunk_size: self.chunk_size,
            align: self.align,
            dry_run: self.dry_run,
            dedup: self.dedup,
            skip_blank: self.skip_blank,
            skip_carved: self.skip_carved,
            jobs: self.jobs.max(1) as usize,
            max_output: self.max_output,
            validate: self.validate,
            drop_failed: self.drop_failed,
            max_validate: self.max_validate,
            ..carver::Options::default()
        }
    }

    fn signature_types(&self) -> Result<Vec<&'static Signature>, ScanError> {
        if self.signature_types.is_empty() {
            return Ok(SIGNATURES.iter().collect());
        }
        resolve_types(&self.signature_types.join(",")).map_err(scan_error)
    }
}

impl Default for CarveOptions {
    fn default() -> Self {
        let core = carver::Options::default();
        CarveOptions {
            out_dir: core.out_dir,
            signature_types: Vec::new(),
            start: core.start,
            length: core.length,
            min_size: core.min_size,
            max_size: core.max_size,
            chunk_size: core.chunk_size,
            align: core.align,
            dry_run: core.dry_run,
            dedup: core.dedup,
            skip_blank: core.skip_blank,
            skip_carved: core.skip_carved,
            jobs: core.jobs as u32,
            max_output: core.max_output,
            validate: core.validate,
            drop_failed: core.drop_failed,
            max_validate: core.max_validate,
        }
    }
}

/// A scan a mid-range phone survives: a smaller chunk and fewer workers,
/// because resident memory is roughly `jobs * chunk_size`, and a lower
/// validation ceiling, because validation buffers the whole carve.
///
/// A plain function rather than a constructor: uniffi only allows those on
/// interfaces, and CarveOptions is a record.
#[uniffi::export]
pub fn mobile_carve_options() -> CarveOptions {
    carver_options_from_core(carver::Options::mobile())
}

fn carver_options_from_core(core: carver::Options) -> CarveOptions {
    CarveOptions {
        out_dir: core.out_dir,
        signature_types: Vec::new(),
        start: core.start,
        length: core.length,
        min_size: core.min_size,
        max_size: core.max_size,
        chunk_size: core.chunk_size,
        align: core.align,
        dry_run: core.dry_run,
        dedup: core.dedup,
        skip_blank: core.skip_blank,
        skip_carved: core.skip_carved,
        jobs: core.jobs as u32,
        max_output: core.max_output,
        validate: core.validate,
        drop_failed: core.drop_failed,
        max_validate: core.max_validate,
    }
}

fn carve_record(r: &carver::Record) -> CarveRecord {
    CarveRecord {
        kind: r.kind.to_string(),
        ext: r.ext.to_string(),
        offset: r.offset,
        size: r.size,
        sha256: r.sha256.clone(),
        validated: r.validated,
        path: r.path.clone(),
        duplicate_of: r.duplicate_of,
        decoded: r.decoded,
        confidence: r.confidence().to_string(),
    }
}

/// Poll a shared progress counter on a timer, reporting through the callback.
///
/// The scan threads only bump counters; a watcher like this is how any front
/// end sees them. The `done` flag is the only reliable exit: a cancelled or
/// budget-halted scan finishes with `scanned` below `total` forever, so a
/// "reached the end" check alone would spin here for the life of the process
/// and hang the scan call at its join.
fn spawn_progress_watcher(
    progress: Arc<carver::Progress>,
    done: Arc<std::sync::atomic::AtomicBool>,
    callback: Arc<dyn ProgressCallback>,
) -> std::thread::JoinHandle<()> {
    use std::sync::atomic::Ordering;
    std::thread::spawn(move || {
        while !done.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(200));
            if done.load(Ordering::Relaxed) {
                break;
            }
            callback.on_progress(
                progress.scanned(),
                progress.total,
                progress.files(),
                progress.bytes_out(),
            );
        }
    })
}

/// Carve files out of an image, the desktop CLI's carving engine.
///
/// The source is opened read-only and never written to; output goes under
/// `options.out_dir`, which must differ from the source's storage (AGENTS.md
/// §3). `progress` receives periodic counter updates while the scan runs;
/// `cancel` stops it early, keeping what was already written.
#[uniffi::export]
pub fn scan_carve(
    source: Arc<SourceHandle>,
    options: CarveOptions,
    progress: Option<Box<dyn ProgressCallback>>,
    cancel: Option<Arc<ScanCancel>>,
) -> Result<Vec<CarveRecord>, ScanError> {
    // Bad names are rejected before the source is even opened.
    let sigs = options.signature_types()?;
    let src = &source.inner;
    let mut opts = options.to_core();
    opts.cancel = cancel.map(|c| Arc::clone(&c.inner));

    let scan_end = if opts.length > 0 {
        (opts.start + opts.length).min(src.size())
    } else {
        src.size()
    };
    let progress_counter = Arc::new(carver::Progress::new(scan_end.saturating_sub(opts.start)));
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let callback: Option<Arc<dyn ProgressCallback>> = progress.map(Arc::from);
    let watcher = callback.as_ref().map(|cb| {
        spawn_progress_watcher(
            Arc::clone(&progress_counter),
            Arc::clone(&done),
            Arc::clone(cb),
        )
    });

    let budget = OutputBudget::for_options(&opts);
    // The flag must clear before any early return, including the error path:
    // an abandoned watcher would keep calling into the foreign callback for
    // the life of the process. One final report goes out from this thread
    // afterwards, so a cancelled scan's last update still lands.
    let result =
        carver::run_parallel_with_budget(src, &sigs, &opts, &budget, Some(&progress_counter));
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Some(cb) = &callback {
        cb.on_progress(
            progress_counter.scanned(),
            progress_counter.total,
            progress_counter.files(),
            progress_counter.bytes_out(),
        );
    }
    let records = result.map_err(scan_error)?;

    if let Some(w) = watcher {
        let _ = w.join();
    }
    Ok(records.iter().map(carve_record).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The CLI suite's minimal JPEG (crates/lazarbyte-cli/tests/builders.rs),
    /// copied rather than imported so the test suite stays independent: SOF0,
    /// a minimal SOS whose entropy data holds a stuffed FF 00, then EOI.
    fn make_jpeg() -> Vec<u8> {
        let mut out = vec![0xFF, 0xD8, 0xFF, 0xE0];
        out.extend_from_slice(b"\x00\x10JFIF\x00\x01\x01\x00\x00\x01\x00\x01\x00\x00");
        out.extend_from_slice(&[
            0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x02, 0x00, 0x02, 0x01, 0x01, 0x11, 0x00,
        ]);
        out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
        out.extend_from_slice(&[0x12, 0x34, 0xFF, 0x00, 0x56, 0xFF, 0x00, 0x78]);
        out.extend_from_slice(&[0xFF, 0xD9]);
        out
    }

    pub(crate) struct Tmp(pub std::path::PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Self {
            let mut p = std::env::temp_dir();
            p.push(format!("lazarbyte-ffi-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Tmp(p)
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_bridge_reports_its_version() {
        assert!(!bridge_version().is_empty());
    }

    #[test]
    fn a_missing_source_is_an_error_not_a_crash() {
        let src = SourceHandle::from_path("Z:/definitely/not/there.img".into());
        assert!(src.is_err(), "a missing source must fail to open");
    }

    #[test]
    fn an_unknown_signature_name_is_rejected_before_any_work() {
        let tmp = Tmp::new("badsig");
        let img = tmp.0.join("img.bin");
        std::fs::write(&img, [0u8; 4096]).unwrap();
        let o = CarveOptions {
            signature_types: vec!["not-a-type".into()],
            ..CarveOptions::default()
        };
        // The source opens, but no scan starts: the name is checked first.
        let src = Arc::new(
            SourceHandle::from_path(img.to_string_lossy().to_string()).expect("open source"),
        );
        let r = scan_carve(src, o, None, None);
        let Err(ScanError::Failed(msg)) = r else {
            panic!("expected a signature error");
        };
        assert!(msg.contains("not-a-type"), "{msg}");
    }

    #[test]
    fn summarize_reports_duplicates_and_counts() {
        let rows = vec![
            TriageRowInput {
                kind: "fat".into(),
                name: "deleted.txt".into(),
                ext: "txt".into(),
                size: 880,
                sha256: "dup".into(),
                validated: true,
                modified: Some(1700000000),
            },
            TriageRowInput {
                kind: "jpg".into(),
                name: "f_1.jpg".into(),
                ext: "jpg".into(),
                size: 880,
                sha256: "dup".into(),
                validated: true,
                modified: None,
            },
            TriageRowInput {
                kind: "fat".into(),
                name: "inner.bin".into(),
                ext: "bin".into(),
                size: 5120,
                sha256: "unique".into(),
                validated: false,
                modified: Some(0),
            },
        ];
        let t = summarize_results(rows);
        assert_eq!(t.total_files, 3);
        assert_eq!(t.total_bytes, 880 + 880 + 5120);
        assert_eq!(t.duplicate_sha256, vec!["dup".to_string()]);
        assert_eq!(t.duplicate_rows, 2);
        assert_eq!(t.by_kind[0].kind, "fat");
        assert_eq!(t.unvalidated, 1);
        assert_eq!(t.modified_min, Some(1700000000));
        assert_eq!(t.modified_max, Some(1700000000));
    }

    #[test]
    fn verify_recovered_hashes_and_compares() {
        use sha2::Digest;
        let tmp = Tmp::new("verify");
        let p = tmp.0.join("f.bin");
        std::fs::write(&p, b"hello").unwrap();
        let sha = format!("{:x}", sha2::Sha256::digest(b"hello"));

        let out = verify_recovered(vec![VerifyInput {
            path: p.to_string_lossy().to_string(),
            sha256: sha,
        }]);
        assert!(out[0].ok);
        assert_eq!(
            out[0].actual_sha256,
            format!("{:x}", sha2::Sha256::digest(b"hello"))
        );

        let out = verify_recovered(vec![VerifyInput {
            path: "Z:/definitely/not/here".into(),
            sha256: "anything".into(),
        }]);
        assert!(!out[0].ok);
        assert!(out[0].actual_sha256.is_empty());
    }

    #[test]
    fn identify_files_reads_content_not_names() {
        let tmp = Tmp::new("identify");
        let jpeg = tmp.0.join("actually_a_jpeg.txt"); // the name lies
        let mut head = vec![0xFF, 0xD8, 0xFF, 0xE0];
        head.extend_from_slice(b" JFIF");
        std::fs::write(&jpeg, &head).unwrap();

        let out = identify_files(vec![jpeg.to_string_lossy().to_string()]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].detected.as_deref(), Some("jpg"));
        assert!(
            !out[0].matches_extension,
            "the .txt name must not claim a match"
        );

        // A text file matches no signature and no extension.
        let plain = tmp.0.join("notes.txt");
        std::fs::write(&plain, b"nothing magical here").unwrap();
        let out = identify_files(vec![plain.to_string_lossy().to_string()]);
        assert!(out[0].detected.is_none());
        assert!(!out[0].matches_extension);
    }

    #[test]
    fn probe_reads_the_volume_family_from_superblocks() {
        let (tmp, img) = undelete_tests::Tmp::image("fat", "exfat.dd.gz");
        let src = Arc::new(SourceHandle::from_path(img.clone()).expect("open source"));
        let p = src.probe();
        assert_eq!(p.size_bytes, 8_388_608);
        assert_eq!(p.fat_kind.as_deref(), Some("exfat"));
        assert!(p.ext_kind.is_none());
        // Dropping the handle is the caller's only cleanup; probe must not
        // have consumed anything.
        drop(src);
        drop(tmp);
    }

    #[test]
    fn carving_through_the_bridge_matches_the_cli_record_shape() {
        let tmp = Tmp::new("carve");
        let img = tmp.0.join("img.bin");
        let jpeg = make_jpeg();
        let mut body = vec![0u8; 4096];
        body[512..512 + jpeg.len()].copy_from_slice(&jpeg);
        std::fs::write(&img, &body).unwrap();

        let o = CarveOptions {
            out_dir: tmp.0.join("out").to_string_lossy().to_string(),
            signature_types: vec!["jpeg".into()],
            ..CarveOptions::default()
        };
        let records = scan_carve(
            Arc::new(
                SourceHandle::from_path(img.to_string_lossy().to_string()).expect("open source"),
            ),
            o,
            None,
            None,
        )
        .unwrap();

        assert_eq!(records.len(), 1, "{records:?}");
        let r = &records[0];
        // The record names the signature, not the CLI's type alias.
        assert_eq!(r.kind, "jpg");
        assert_eq!(r.offset, 512);
        assert_eq!(r.size, jpeg.len() as u64);
        assert!(r.validated);
        assert_eq!(r.confidence, "high");
        assert!(std::path::Path::new(&r.path).is_file(), "{}", r.path);
    }

    #[test]
    fn a_cancelled_scan_stops_and_reports_what_it_wrote() {
        let tmp = Tmp::new("cancel");
        let img = tmp.0.join("img.bin");
        // One megabyte of zeros: nothing to find, but enough to scan slowly.
        std::fs::write(&img, vec![0u8; 1 << 20]).unwrap();
        let cancel = Arc::new(ScanCancel::new());
        cancel.cancel();

        let o = CarveOptions {
            out_dir: tmp.0.join("out").to_string_lossy().to_string(),
            ..CarveOptions::default()
        };
        let records = scan_carve(
            Arc::new(
                SourceHandle::from_path(img.to_string_lossy().to_string()).expect("open source"),
            ),
            o,
            None,
            Some(cancel),
        );
        // A cancelled scan is not an error: it returns what it has, here
        // nothing, because zeros hold no signatures.
        assert!(records.is_ok(), "{records:?}");
        assert!(records.unwrap().is_empty());
    }

    #[test]
    fn a_cancelled_scan_with_progress_returns_promptly() {
        // Regression: the progress watcher used to exit only when scanned
        // reached total, which a cancelled scan never does -- the call then
        // hung at its join forever, and the app's cancel looked broken. With
        // the done flag, the call returns promptly with partial results.
        let tmp = Tmp::new("cancel-progress");
        let img = tmp.0.join("img.bin");
        std::fs::write(&img, vec![0u8; 1 << 20]).unwrap();
        let cancel = Arc::new(ScanCancel::new());
        cancel.cancel();

        struct Fired(Arc<std::sync::atomic::AtomicBool>);
        impl ProgressCallback for Fired {
            fn on_progress(&self, _scanned: u64, _total: u64, _files: u64, _bytes_out: u64) {
                self.0.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let fired = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cb: Box<dyn ProgressCallback> = Box::new(Fired(Arc::clone(&fired)));

        let o = CarveOptions {
            out_dir: tmp.0.join("out").to_string_lossy().to_string(),
            ..CarveOptions::default()
        };
        let started = std::time::Instant::now();
        let records = scan_carve(
            Arc::new(
                SourceHandle::from_path(img.to_string_lossy().to_string()).expect("open source"),
            ),
            o,
            Some(cb),
            Some(cancel),
        )
        .expect("a cancelled scan is not an error");
        assert!(records.is_empty());
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "the watcher kept the call alive: {:?}",
            started.elapsed()
        );
        assert!(fired.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn the_progress_callback_sees_the_scan_move() {
        let tmp = Tmp::new("progress");
        let img = tmp.0.join("img.bin");
        std::fs::write(&img, make_jpeg()).unwrap();

        // A `Box<dyn ProgressCallback>` is `'static`, so the shared log goes
        // behind an Arc and the test reads it after the scan returns.
        struct Seen(Arc<std::sync::Mutex<Vec<(u64, u64)>>>);
        impl ProgressCallback for Seen {
            fn on_progress(&self, scanned: u64, total: u64, _files: u64, _bytes_out: u64) {
                self.0.lock().unwrap().push((scanned, total));
            }
        }
        let log = Arc::new(std::sync::Mutex::new(Vec::new()));

        let o = CarveOptions {
            out_dir: tmp.0.join("out").to_string_lossy().to_string(),
            ..CarveOptions::default()
        };
        let cb: Box<dyn ProgressCallback> = Box::new(Seen(Arc::clone(&log)));
        let records = scan_carve(
            Arc::new(
                SourceHandle::from_path(img.to_string_lossy().to_string()).expect("open source"),
            ),
            o,
            Some(cb),
            None,
        )
        .unwrap();
        assert!(!records.is_empty());

        let reports = log.lock().unwrap();
        assert!(
            !reports.is_empty(),
            "the watcher never fired across the scan"
        );
        let last = reports.last().unwrap();
        assert_eq!(last.0, last.1, "the scan ran to completion: {reports:?}");
    }
}

// --- undelete exports (Faz 4, step 3) ---

/// A file's timestamps, in seconds since the Unix epoch.
///
/// Engines differ in what they keep: FAT has no change or delete times, ext
/// has no creation time. A field the engine does not carry is `0`, which the
/// app renders as "unknown".
#[derive(Debug, uniffi::Record)]
pub struct Timestamps {
    pub created: u64,
    pub modified: u64,
    pub accessed: u64,
    /// When the content last changed (ext, NTFS). `0` when not carried.
    pub changed: u64,
    /// When the file was freed (ext keeps this, unlike NTFS). `0` when not.
    pub deleted: u64,
}

fn timestamps_fat(ts: lazarbyte_core::fat::Timestamps) -> Timestamps {
    Timestamps {
        created: ts.created,
        modified: ts.modified,
        accessed: ts.accessed,
        changed: 0,
        deleted: 0,
    }
}

fn timestamps_ext4(ts: lazarbyte_core::ext4::Timestamps) -> Timestamps {
    Timestamps {
        created: 0,
        modified: ts.modified,
        accessed: ts.accessed,
        changed: ts.changed,
        deleted: ts.deleted,
    }
}

/// Settings shared by the undelete engines.
///
/// A SAF-backed sink arrives with the Android bridge; until then `out_dir`
/// must be a writable directory path, always a different one from the source's
/// storage: the source is opened read-only and never written to.
#[derive(Debug, uniffi::Record)]
pub struct UndeleteOptions {
    /// Where recovered files go.
    pub out_dir: String,
    /// Byte offset of the volume inside the image; `0` lets the engine find it.
    pub offset: u64,
    /// Also report files that are still there, not just deleted ones.
    pub include_live: bool,
    /// Skip files smaller than this.
    pub min_size: u64,
}

/// One file recovered from a FAT12/16/32 or exFAT volume.
#[derive(Debug, uniffi::Record)]
pub struct FatFileRecord {
    /// `fat` or `exfat`.
    pub kind: String,
    pub name: String,
    pub ext: String,
    /// Byte offset of the directory entry this came from.
    pub offset: u64,
    pub size: u64,
    pub sha256: String,
    /// The declared length was readable inside the volume. It does not mean
    /// the file was contiguous -- FAT cannot say.
    pub validated: bool,
    pub deleted: bool,
    pub path: String,
    pub timestamps: Timestamps,
    /// The core's confidence wording: high / low.
    pub confidence: String,
}

/// What a FAT undelete run saw, on top of the records.
#[derive(Debug, uniffi::Record)]
pub struct FatResult {
    pub records: Vec<FatFileRecord>,
    /// `fat`, `fat16`, `fat32` or `exfat`, as the engine identified it.
    pub kind: String,
    pub cluster_size: u64,
}

#[uniffi::export]
pub fn undelete_fat(
    source: Arc<SourceHandle>,
    options: UndeleteOptions,
    cancel: Option<Arc<ScanCancel>>,
) -> Result<FatResult, ScanError> {
    let src = &source.inner;
    // dry_run stays false: a dry run is a CLI inventory mode, the app wants
    // the files themselves. A sink-less options leaves the FsSink default.
    let opts = lazarbyte_core::fat::Options {
        out_dir: options.out_dir,
        dry_run: false,
        include_live: options.include_live,
        min_size: options.min_size,
        cancel: cancel.map(|c| Arc::clone(&c.inner)),
        sink: None,
    };
    let (records, kind, cluster_size) =
        lazarbyte_core::fat::recover(src, options.offset, &opts, |_| {}).map_err(scan_error)?;
    Ok(FatResult {
        records: records
            .iter()
            .map(|r| FatFileRecord {
                kind: r.kind.to_string(),
                name: r.name.clone(),
                ext: r.ext.clone(),
                offset: r.offset,
                size: r.size,
                sha256: r.sha256.clone(),
                validated: r.validated,
                deleted: r.deleted,
                path: r.path.clone(),
                timestamps: timestamps_fat(r.timestamps),
                confidence: r.confidence().to_string(),
            })
            .collect(),
        kind: kind.to_string(),
        cluster_size,
    })
}

/// One file recovered from an ext2/3/4 volume.
#[derive(Debug, uniffi::Record)]
pub struct Ext4FileRecord {
    pub inode: u64,
    /// Path inside the volume, or `#inode_N` when no name was found.
    pub name: String,
    pub ext: String,
    pub size: u64,
    pub sha256: String,
    /// Whole only when every block was read and the name was found.
    pub validated: bool,
    pub deleted: bool,
    pub path: String,
    pub timestamps: Timestamps,
    /// The core's confidence wording: high / low.
    pub confidence: String,
}

/// What an ext4 undelete run saw, on top of the records.
#[derive(Debug, uniffi::Record)]
pub struct Ext4Result {
    pub records: Vec<Ext4FileRecord>,
    pub block_size: u64,
    pub inodes: u64,
    pub volume_size: u64,
    /// Deleted inodes whose block map was already cleared: named but gone.
    pub map_gone: u64,
}

#[uniffi::export]
pub fn undelete_ext4(
    source: Arc<SourceHandle>,
    options: UndeleteOptions,
    cancel: Option<Arc<ScanCancel>>,
) -> Result<Ext4Result, ScanError> {
    let src = &source.inner;
    let opts = lazarbyte_core::ext4::Options {
        out_dir: options.out_dir,
        dry_run: false,
        include_live: options.include_live,
        min_size: options.min_size,
        cancel: cancel.map(|c| Arc::clone(&c.inner)),
        sink: None,
    };
    let (records, summary) =
        lazarbyte_core::ext4::recover(src, options.offset, &opts, |_| {}).map_err(scan_error)?;
    Ok(Ext4Result {
        records: records
            .iter()
            .map(|r| Ext4FileRecord {
                inode: r.inode,
                name: r.name.clone(),
                ext: r.ext.clone(),
                size: r.size,
                sha256: r.sha256.clone(),
                validated: r.validated,
                deleted: r.deleted,
                path: r.path.clone(),
                timestamps: timestamps_ext4(r.timestamps),
                confidence: r.confidence().to_string(),
            })
            .collect(),
        block_size: summary.block_size,
        inodes: summary.inodes,
        volume_size: summary.volume_size,
        map_gone: summary.map_gone,
    })
}

#[cfg(test)]
mod undelete_tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::io::Read;

    pub(crate) struct Tmp(std::path::PathBuf);

    impl Tmp {
        /// Unpack a fixture next to the output directory the scan writes to.
        pub(crate) fn image(dir_kind: &str, name: &str) -> (Self, String) {
            let mut dir = std::env::temp_dir();
            // The fixture name keeps parallel tests out of one another's way:
            // two tests unpacking the same volume otherwise share a directory.
            // A sequence number as well: two tests can unpack the same
            // fixture, and only the process id would leave them racing.
            use std::sync::atomic::{AtomicU64, Ordering};
            static SEQ: AtomicU64 = AtomicU64::new(0);
            dir.push(format!(
                "lazarbyte-ffi-{}-{}-{}-{}",
                dir_kind,
                name.trim_end_matches(".gz")
                    .trim_end_matches(".dd")
                    .trim_end_matches(".img"),
                std::process::id(),
                SEQ.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            let gz = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../lazarbyte-cli/tests/fixtures")
                .join(dir_kind)
                .join(name);
            let mut data = Vec::new();
            GzDecoder::new(
                std::fs::File::open(&gz).unwrap_or_else(|e| panic!("{}: {e}", gz.display())),
            )
            .read_to_end(&mut data)
            .expect("fixture did not decompress");
            let img = dir.join("disk.dd");
            std::fs::write(&img, &data).unwrap();
            (Tmp(dir), img.to_string_lossy().to_string())
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn opts(out_dir: &std::path::Path) -> UndeleteOptions {
        UndeleteOptions {
            out_dir: out_dir.join("out").to_string_lossy().to_string(),
            offset: 0,
            include_live: false,
            min_size: 0,
        }
    }

    #[test]
    fn undelete_fat_through_the_bridge_recovers_the_deleted_files() {
        // exFAT, because it clears an in-use bit instead of overwriting a
        // character, so the long name survives: the FAT16 variant only keeps
        // a short name whose first letter the delete clobbered.
        let (tmp, img) = Tmp::image("fat", "exfat.dd.gz");
        let r = undelete_fat(
            Arc::new(SourceHandle::from_path(img.clone()).expect("open source")),
            opts(&tmp.0),
            None,
        )
        .expect("undelete_fat");
        assert_eq!(r.kind, "exfat");
        assert!(r.records.iter().all(|rec| rec.deleted));
        let recovered = r
            .records
            .iter()
            .find(|rec| rec.name == "deleted.txt" && rec.size == 880)
            .expect("the deleted file must be found by name");
        assert!(recovered.validated, "{:?}", recovered);
        assert_eq!(recovered.confidence, "high");
        assert!(
            std::path::Path::new(&recovered.path).is_file(),
            "{}",
            recovered.path
        );
        assert_eq!(
            std::fs::read(&recovered.path).unwrap(),
            b"deleted evidence text\n".repeat(40)
        );
    }

    #[test]
    fn undelete_ext4_through_the_bridge_recovers_the_deleted_files() {
        let (tmp, img) = Tmp::image("ext", "ext4.img.gz");
        let r = undelete_ext4(
            Arc::new(SourceHandle::from_path(img.clone()).expect("open source")),
            opts(&tmp.0),
            None,
        )
        .expect("undelete_ext4");
        assert!(r.records.iter().all(|rec| rec.deleted));
        let recovered = r
            .records
            .iter()
            .find(|rec| rec.name == "deleted.txt")
            .unwrap();
        assert!(recovered.validated, "{:?}", recovered);
        assert!(
            recovered.timestamps.deleted > 0,
            "ext keeps the delete time"
        );
        assert!(
            std::path::Path::new(&recovered.path).is_file(),
            "{}",
            recovered.path
        );
        assert_eq!(
            std::fs::read(&recovered.path).unwrap(),
            b"deleted ext evidence\n".repeat(44)
        );
    }

    #[test]
    fn a_cancelled_undelete_returns_without_writing_more() {
        let (tmp, img) = Tmp::image("fat", "fat16.dd.gz");
        let cancel = Arc::new(ScanCancel::new());
        cancel.cancel();
        let r = undelete_fat(
            Arc::new(SourceHandle::from_path(img.clone()).expect("open source")),
            opts(&tmp.0),
            Some(cancel),
        )
        .expect("undelete_fat");
        // Nothing had been written yet when the flag was checked, so the run
        // comes back short but not broken.
        assert!(r.records.is_empty(), "{:?}", r.records);
    }

    #[test]
    fn undelete_rejects_an_unreadable_volume_with_a_clear_error() {
        let mut dir = std::env::temp_dir();
        dir.push(format!("lazarbyte-ffi-badvol-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let img = dir.join("garbage.bin");
        let img = img.to_string_lossy().to_string();
        std::fs::write(&img, [0x41u8; 65536]).unwrap();
        let out = dir.join("out").to_string_lossy().to_string();
        for r in [
            undelete_fat(
                Arc::new(SourceHandle::from_path(img.clone()).expect("open source")),
                UndeleteOptions {
                    out_dir: out.clone(),
                    offset: 0,
                    include_live: false,
                    min_size: 0,
                },
                None,
            )
            .expect_err("fat must reject"),
            undelete_ext4(
                Arc::new(SourceHandle::from_path(img).expect("open source")),
                UndeleteOptions {
                    out_dir: out,
                    offset: 0,
                    include_live: false,
                    min_size: 0,
                },
                None,
            )
            .expect_err("ext4 must reject"),
        ] {
            let ScanError::Failed(msg) = r;
            assert!(
                msg.to_lowercase().contains("fat") || msg.to_lowercase().contains("ext"),
                "{msg}"
            );
        }
    }
}

// --- triage and re-verification exports (backlog P0) ---

/// One row of a scan result, normalized across engines, as the triage input.
#[derive(Debug, uniffi::Record)]
pub struct TriageRowInput {
    pub kind: String,
    pub name: String,
    pub ext: String,
    pub size: u64,
    pub sha256: String,
    pub validated: bool,
    /// Last modification time when the engine knows it (undelete modes);
    /// `None` for carved files, which carry no timestamps.
    pub modified: Option<u64>,
}

/// Files per format family.
#[derive(Debug, uniffi::Record)]
pub struct KindCount {
    pub kind: String,
    pub files: u64,
    pub bytes: u64,
}

/// One of the biggest recovered files.
#[derive(Debug, uniffi::Record)]
pub struct LargestFile {
    pub name: String,
    pub kind: String,
    pub size: u64,
}

/// The shape of the accumulated scan results.
#[derive(Debug, uniffi::Record)]
pub struct TriageSummary {
    pub total_files: u64,
    pub total_bytes: u64,
    /// Per format family, largest total first.
    pub by_kind: Vec<KindCount>,
    /// Up to the five biggest files.
    pub largest: Vec<LargestFile>,
    /// SHA-256 values found on more than one row: byte-identical files a
    /// quick scan and a deep scan both found, for example.
    pub duplicate_sha256: Vec<String>,
    pub duplicate_rows: u64,
    /// Rows whose extension the signature table does not recognise.
    pub unknown_extensions: u64,
    /// Modified-time span over rows that carry a time; `None` when none do.
    pub modified_min: Option<u64>,
    pub modified_max: Option<u64>,
    /// Rows the engines could not fully account for.
    pub unvalidated: u64,
}

/// Summarize accumulated scan results: per-kind counts, biggest files,
/// byte-identical duplicates, unrecognized extensions, the modified-time
/// span and the unvalidated count. Pure arithmetic over the rows; no I/O.
#[uniffi::export]
pub fn summarize_results(rows: Vec<TriageRowInput>) -> TriageSummary {
    let known = lazarbyte_core::signatures::known_extensions();
    let core_rows: Vec<lazarbyte_core::triage::TriageRow> = rows
        .into_iter()
        .map(|r| lazarbyte_core::triage::TriageRow {
            kind: r.kind,
            name: r.name,
            ext: r.ext,
            size: r.size,
            sha256: r.sha256,
            validated: r.validated,
            modified: r.modified.filter(|m| *m != 0),
        })
        .collect();
    let t = lazarbyte_core::triage::summarize(&core_rows, &known);
    TriageSummary {
        total_files: t.total_files,
        total_bytes: t.total_bytes,
        by_kind: t
            .by_kind
            .into_iter()
            .map(|k| KindCount {
                kind: k.kind,
                files: k.files,
                bytes: k.bytes,
            })
            .collect(),
        largest: t
            .largest
            .into_iter()
            .map(|l| LargestFile {
                name: l.name,
                kind: l.kind,
                size: l.size,
            })
            .collect(),
        duplicate_sha256: t.duplicate_sha256,
        duplicate_rows: t.duplicate_rows,
        unknown_extensions: t.unknown_extensions,
        modified_min: t.modified_min,
        modified_max: t.modified_max,
        unvalidated: t.unvalidated,
    }
}

/// One re-verification result: the file's actual SHA-256 against what the
/// manifest claims.
#[derive(Debug, uniffi::Record)]
pub struct VerifyOutcome {
    pub path: String,
    /// The file was readable and its hash matches the manifest.
    pub ok: bool,
    /// What was actually hashed; empty when the file was unreadable.
    pub actual_sha256: String,
}

/// Hash every given recovered file again and compare with the manifest's
/// SHA-256. This is the chain-of-custody check: the bytes on disk still are
/// the bytes the scan reported. Reads in chunks, so a large file costs no
/// more memory than a small one.
#[uniffi::export]
pub fn verify_recovered(items: Vec<VerifyInput>) -> Vec<VerifyOutcome> {
    use sha2::Digest;
    items
        .into_iter()
        .map(|item| {
            let mut hasher = sha2::Sha256::new();
            let read = std::fs::File::open(&item.path).and_then(|mut f| {
                std::io::copy(&mut f, &mut hasher)?;
                Ok(())
            });
            match read {
                Ok(()) => {
                    let actual = format!("{:x}", hasher.finalize());
                    VerifyOutcome {
                        path: item.path,
                        ok: actual == item.sha256,
                        actual_sha256: actual,
                    }
                }
                Err(_) => VerifyOutcome {
                    path: item.path,
                    ok: false,
                    actual_sha256: String::new(),
                },
            }
        })
        .collect()
}

/// One re-verification request: where the file is and what the manifest
/// claims its SHA-256 is.
#[derive(Debug, uniffi::Record)]
pub struct VerifyInput {
    pub path: String,
    pub sha256: String,
}

// --- identify and probe exports (backlog P1) ---

/// What a recovered file's bytes say its type is, versus what its name says.
#[derive(Debug, uniffi::Record)]
pub struct IdentifyOutcome {
    pub path: String,
    /// The signature family the leading bytes matched, if any.
    pub detected: Option<String>,
    /// The name's extension agrees with the detected content.
    pub matches_extension: bool,
}

/// Identify a batch of recovered files from their leading bytes. Rows whose
/// bytes match no signature get `detected = None` -- a carved fragment, or a
/// type this table lacks; the row says so rather than guessing.
#[uniffi::export]
pub fn identify_files(paths: Vec<String>) -> Vec<IdentifyOutcome> {
    use std::io::Read;
    paths
        .into_iter()
        .map(|path| {
            let head = std::fs::File::open(&path).and_then(|mut f| {
                let mut buf = vec![0u8; 4096];
                let mut got = 0;
                loop {
                    let n = f.read(&mut buf[got..])?;
                    if n == 0 {
                        break;
                    }
                    got += n;
                    if got == buf.len() {
                        break;
                    }
                }
                buf.truncate(got);
                Ok(buf)
            });
            match head {
                Ok(head) => {
                    let detected = lazarbyte_core::signatures::identify_head(&head)
                        .map(|sig| sig.name.to_string());
                    // The name's extension agrees when it resolves to the same
                    // signature family: an alias (jpeg) counts as its target
                    // (jpg), anything else does not.
                    let ext = std::path::Path::new(&path)
                        .extension()
                        .map(|e| e.to_string_lossy().to_ascii_lowercase())
                        .unwrap_or_default();
                    let matches_extension = match &detected {
                        None => false,
                        Some(family) => {
                            let normalized = lazarbyte_core::signatures::resolve_types(&ext)
                                .ok()
                                .and_then(|sigs| sigs.first().map(|s| s.name.to_string()));
                            normalized.as_deref() == Some(family.as_str())
                        }
                    };
                    IdentifyOutcome {
                        path,
                        detected,
                        matches_extension,
                    }
                }
                Err(_) => IdentifyOutcome {
                    path,
                    detected: None,
                    matches_extension: false,
                },
            }
        })
        .collect()
}

/// What a quick look at an open image can say before a scan: its size and
/// the filesystem family found at its start, if any. Cheap -- it reads the
/// volume superblock, not the bytes.
#[derive(Debug, uniffi::Record)]
pub struct ProbeInfo {
    pub size_bytes: u64,
    /// `fat`/`exfat` when an FAT family superblock sits at the start.
    pub fat_kind: Option<String>,
    /// `ext2/3/4` when an ext superblock sits at the start.
    pub ext_kind: Option<String>,
}

#[uniffi::export]
impl SourceHandle {
    /// Pre-scan probe: filesystem family and size, from superblocks only.
    pub fn probe(&self) -> ProbeInfo {
        let fat = lazarbyte_core::fat::locate(&self.inner, 0)
            .ok()
            .map(|vol| vol.kind_name().to_string());
        // The ext volume does not carry a family field of its own; the
        // engine reports the whole ext2/3/4 family.
        let ext = lazarbyte_core::ext4::locate(&self.inner, 0)
            .map(|_| "ext2/3/4".to_string())
            .ok();
        ProbeInfo {
            size_bytes: self.inner.size(),
            fat_kind: fat,
            ext_kind: ext,
        }
    }
}
