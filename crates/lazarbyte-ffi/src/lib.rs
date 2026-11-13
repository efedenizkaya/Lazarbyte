//! uniffi bindings over `lazarbyte-core`.
//!
//! The engines live in `lazarbyte-core`; this crate only adapts them to the
//! types uniffi can cross the FFI boundary with. Business logic stays out: an
//! exported function parses arguments, calls one core entry point and maps its
//! `Result` to an error the foreign side can catch.
//!
//! The whole crate is unsafe-free. The FdSource that will wrap a Kotlin
//! `ParcelFileDescriptor` is the one approved exception to that rule
//! (AGENTS.md §10) and lands with the Android build, not before.

use std::sync::Arc;
use std::time::Duration;

use lazarbyte_core::carver::{self, OutputBudget};
use lazarbyte_core::reader::Source;
use lazarbyte_core::signatures::{resolve_types, Signature, SIGNATURES};

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

impl CarveOptions {
    /// A scan a mid-range phone survives: a smaller chunk and fewer workers,
    /// because resident memory is roughly `jobs * chunk_size`, and a lower
    /// validation ceiling, because validation buffers the whole carve.
    pub fn mobile() -> Self {
        let core = carver::Options::mobile();
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
/// end sees them. It stops when the sender's flag drops, which happens when
/// the scan call returns.
fn spawn_progress_watcher(
    progress: Arc<carver::Progress>,
    callback: Box<dyn ProgressCallback>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(200));
            callback.on_progress(
                progress.scanned(),
                progress.total,
                progress.files(),
                progress.bytes_out(),
            );
            // A final report would be nice, but the scan's own return value is
            // the authoritative end: the caller moves on once it is back.
            if progress.scanned() >= progress.total {
                break;
            }
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
    source_path: String,
    options: CarveOptions,
    progress: Option<Box<dyn ProgressCallback>>,
    cancel: Option<Arc<ScanCancel>>,
) -> Result<Vec<CarveRecord>, ScanError> {
    // Bad names are rejected before the source is even opened.
    let sigs = options.signature_types()?;
    let src = Source::open(&source_path).map_err(|e| scan_error(e.to_string()))?;
    let mut opts = options.to_core();
    opts.cancel = cancel.map(|c| Arc::clone(&c.inner));

    let scan_end = if opts.length > 0 {
        (opts.start + opts.length).min(src.size())
    } else {
        src.size()
    };
    let progress_counter = Arc::new(carver::Progress::new(scan_end.saturating_sub(opts.start)));
    let watcher = progress.map(|cb| spawn_progress_watcher(Arc::clone(&progress_counter), cb));

    let budget = OutputBudget::for_options(&opts);
    let records =
        carver::run_parallel_with_budget(&src, &sigs, &opts, &budget, Some(&progress_counter))
            .map_err(scan_error)?;

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

    struct Tmp(std::path::PathBuf);

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
        let r = scan_carve(
            "Z:/definitely/not/there.img".into(),
            CarveOptions::default(),
            None,
            None,
        );
        assert!(r.is_err());
    }

    #[test]
    fn an_unknown_signature_name_is_rejected_before_any_work() {
        let o = CarveOptions {
            signature_types: vec!["not-a-type".into()],
            ..CarveOptions::default()
        };
        let r = scan_carve("unused".into(), o, None, None);
        let Err(ScanError::Failed(msg)) = r else {
            panic!("expected a signature error");
        };
        assert!(msg.contains("not-a-type"), "{msg}");
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
        let records = scan_carve(img.to_string_lossy().to_string(), o, None, None).unwrap();

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
        let records = scan_carve(img.to_string_lossy().to_string(), o, None, Some(cancel));
        // A cancelled scan is not an error: it returns what it has, here
        // nothing, because zeros hold no signatures.
        assert!(records.is_ok(), "{records:?}");
        assert!(records.unwrap().is_empty());
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
        let records = scan_carve(img.to_string_lossy().to_string(), o, Some(cb), None).unwrap();
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
