//! Where recovered files are written.
//!
//! The desktop names a directory and lets the core name the file. Android
//! cannot: the Storage Access Framework hands out a document per file, not a
//! path, so the core asks a sink for a writer and stores whatever identifier
//! the sink returns. On desktop that identifier is the path a record has always
//! carried, so manifests are unchanged.
//!
//! A sink is chosen per run (`carver::Options::sink` and the same field on each
//! undelete `Options`); the default is [`FsSink`] over `Options::out_dir`.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// A destination for recovered files.
///
/// The three operations are the only ones the engines need: write a new file,
/// write one that must not overwrite, and drop one that turned out to be a
/// duplicate. Sinks are shared by scan workers, so they must be thread-safe.
pub trait Sink: Send + Sync {
    /// Create `rel` (a path relative to the sink root, always with `/`
    /// separators) and return its writer and the identifier for the record.
    ///
    /// This may replace an existing file: a resumed scan carves the same
    /// offset again, and rewriting the same bytes under the same name is what
    /// keeps a resume from filling the output with copies.
    fn create(&self, rel: &Path) -> io::Result<(Box<dyn Write>, String)>;

    /// Create a file that must not replace anything.
    ///
    /// Undelete reports names found in a filesystem's metadata, and two deleted
    /// entries can carry the same name. `alt` is the caller's fallback name
    /// (the entry's offset or inode id, so it is stable across runs); the sink
    /// uses it when `rel` is taken, and picks its own name only if that is
    /// taken too.
    fn create_unique(&self, rel: &Path, alt: &Path) -> io::Result<(Box<dyn Write>, String)>;

    /// Remove a file this sink created, by the identifier it returned.
    ///
    /// Used for duplicates and for carves that turn out to sit inside a
    /// verified one. Failure is not interesting to the caller: the record is
    /// dropped either way, and a stray duplicate is not a lost file.
    fn remove(&self, id: &str);
}

/// The sink a run writes through: the caller's, or a directory on the local
/// filesystem under `dir`.
pub(crate) fn for_run(sink: Option<&Arc<dyn Sink>>, dir: &str) -> Arc<dyn Sink> {
    sink.cloned().unwrap_or_else(|| Arc::new(FsSink::new(dir)))
}

/// The caller's fallback name when a sink already holds `rel`:
/// `dir/stem_TAG.ext`. Both undelete modes need one, and both want it stable
/// across runs, so the tag is the entry's offset or its inode number.
pub(crate) fn alt_name(rel: &Path, tag: &str) -> PathBuf {
    let stem = rel
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let suffix = rel
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    rel.with_file_name(format!("{stem}_{tag}{suffix}"))
}

fn make_parent(p: &Path) -> io::Result<()> {
    match p.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => std::fs::create_dir_all(dir),
        _ => Ok(()),
    }
}

fn new_file(p: &Path) -> io::Result<(Box<dyn Write>, String)> {
    make_parent(p)?;
    let f = std::fs::File::create(p)?;
    Ok((
        Box::new(io::BufWriter::new(f)),
        p.to_string_lossy().to_string(),
    ))
}

fn new_unique_file(p: &Path) -> io::Result<Option<(Box<dyn Write>, String)>> {
    make_parent(p)?;
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(p)
    {
        Ok(f) => Ok(Some((
            Box::new(io::BufWriter::new(f)),
            p.to_string_lossy().to_string(),
        ))),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(None),
        Err(e) => Err(e),
    }
}

/// A directory on a filesystem; the identifier is the file's path.
pub struct FsSink {
    root: PathBuf,
}

impl FsSink {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        FsSink { root: root.into() }
    }

    fn at(&self, rel: &Path) -> PathBuf {
        self.root.join(rel)
    }
}

impl Sink for FsSink {
    fn create(&self, rel: &Path) -> io::Result<(Box<dyn Write>, String)> {
        new_file(&self.at(rel))
    }

    fn create_unique(&self, rel: &Path, alt: &Path) -> io::Result<(Box<dyn Write>, String)> {
        for candidate in [rel, alt] {
            if let Some(w) = new_unique_file(&self.at(candidate))? {
                return Ok(w);
            }
        }
        // Both names are taken: number the fallback rather than overwrite.
        let stem = alt
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        let ext = alt
            .extension()
            .map(|e| e.to_string_lossy().to_string())
            .unwrap_or_default();
        for n in 1..=9999u32 {
            let name = if ext.is_empty() {
                format!("{stem}_{n}")
            } else {
                format!("{stem}_{n}.{ext}")
            };
            let p = self.at(&alt.with_file_name(name));
            if let Some(w) = new_unique_file(&p)? {
                return Ok(w);
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("no free name for {}", alt.display()),
        ))
    }

    fn remove(&self, id: &str) {
        let _ = std::fs::remove_file(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("lazarbyte-sink-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn put(sink: &dyn Sink, rel: &Path, body: &[u8]) -> String {
        let (mut w, id) = sink.create(rel).expect("create");
        w.write_all(body).expect("write");
        w.flush().expect("flush");
        id
    }

    #[test]
    fn create_replaces_what_was_there() {
        let root = scratch("overwrite");
        let sink = FsSink::new(&root);
        let rel = Path::new("png").join("a.png");
        put(&sink, &rel, b"first");
        let id = put(&sink, &rel, b"second");
        assert_eq!(std::fs::read(&id).expect("read"), b"second");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn create_unique_never_overwrites() {
        let root = scratch("unique");
        let sink = FsSink::new(&root);
        let rel = Path::new("fat").join("deleted.txt");
        let alt = Path::new("fat").join("deleted_1f4.txt");

        let first = put(&sink, &rel, b"one");
        let (mut w, second) = sink.create_unique(&rel, &alt).expect("unique");
        w.write_all(b"two").expect("write");
        w.flush().expect("flush");
        let third = {
            let (mut w, id) = sink.create_unique(&rel, &alt).expect("unique");
            w.write_all(b"three").expect("write");
            w.flush().expect("flush");
            id
        };

        assert_eq!(std::fs::read(&first).expect("read"), b"one");
        assert_eq!(std::fs::read(&second).expect("read"), b"two");
        assert_eq!(std::fs::read(&third).expect("read"), b"three");
        assert_ne!(first, second);
        assert_ne!(second, third);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn remove_deletes_by_identifier() {
        let root = scratch("remove");
        let sink = FsSink::new(&root);
        let id = put(&sink, Path::new("x.bin"), b"body");
        assert!(Path::new(&id).exists());
        sink.remove(&id);
        assert!(!Path::new(&id).exists());
        sink.remove(&id); // already gone: not an error
        let _ = std::fs::remove_dir_all(&root);
    }
}
