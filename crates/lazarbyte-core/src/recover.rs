//! Streaming output for the undelete modes.
//!
//! A deleted file's size comes from metadata that may be stale or corrupt, and
//! an undelete on a phone can hand back a multi-gigabyte file. Reading the
//! whole thing into a `Vec` and then writing it means the peak memory is the
//! file's size, which is how an app gets killed half way through. These helpers
//! walk the file's pieces, write as they read, and hash in the same pass.

use crate::reader::Source;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::Path;

/// One piece of a file's content, in file order.
pub enum Piece {
    /// Bytes at this offset in the source.
    Data(u64, u64),
    /// A range the map does not cover; written as zeros and reported.
    Hole(u64),
}

/// Read chunk size. Big enough to keep the syscall count low, small enough that
/// a few workers at once stay within a phone's memory.
const CHUNK: u64 = 8 << 20;

pub struct Written {
    /// Bytes handed to the hasher (always `expected` when the call succeeds).
    pub bytes: u64,
    pub sha256: String,
    /// Every byte came from the source, not from a pad or a hole.
    pub complete: bool,
}

/// Copy `pieces` to `dest`, hashing while it reads.
///
/// `expected` is the file's declared size: a short map is zero-padded to it and
/// reported incomplete, as a whole-file read did before. `None` hashes without
/// writing, which is what a dry run needs. Returns `None` when nothing could be
/// read (no empty file is left behind) or the destination cannot be written.
pub fn copy(src: &Source, pieces: &[Piece], expected: u64, dest: Option<&Path>) -> Option<Written> {
    let mut hasher = Sha256::new();
    let mut bytes = 0u64;
    let mut complete = true;
    let mut out: Option<std::io::BufWriter<std::fs::File>> = None;
    let zeros = vec![0u8; (1 << 20).min(expected.max(1) as usize)];
    let mut stopped = false;

    for piece in pieces {
        if bytes >= expected || stopped {
            break;
        }
        match *piece {
            Piece::Hole(n) => {
                let left = n.min(expected - bytes);
                let mut done = 0u64;
                while done < left {
                    let take = (left - done).min(zeros.len() as u64) as usize;
                    if out.is_none() {
                        if let Some(path) = dest {
                            out = Some(std::io::BufWriter::new(std::fs::File::create(path).ok()?));
                        }
                    }
                    if let Some(w) = out.as_mut() {
                        if w.write_all(&zeros[..take]).is_err() {
                            return None;
                        }
                    }
                    hasher.update(&zeros[..take]);
                    done += take as u64;
                }
                bytes += done;
                complete = false;
            }
            Piece::Data(at, n) => {
                let left = n.min(expected - bytes);
                let mut done = 0u64;
                while done < left {
                    let want = (left - done).min(CHUNK) as usize;
                    let chunk = src.pread(at.saturating_add(done), want);
                    if chunk.is_empty() {
                        complete = false;
                        stopped = true;
                        break;
                    }
                    if out.is_none() {
                        if let Some(path) = dest {
                            out = Some(std::io::BufWriter::new(std::fs::File::create(path).ok()?));
                        }
                    }
                    if let Some(w) = out.as_mut() {
                        if w.write_all(&chunk).is_err() {
                            return None;
                        }
                    }
                    hasher.update(&chunk);
                    done += chunk.len() as u64;
                }
                bytes += done;
                if done < left {
                    complete = false;
                    stopped = true;
                }
            }
        }
    }

    // A declared size past what the map covers was padded before, and the
    // record said so; keep that contract.
    if bytes > 0 && bytes < expected {
        complete = false;
        let mut left = expected - bytes;
        while left > 0 {
            let take = left.min(zeros.len() as u64) as usize;
            if let Some(w) = out.as_mut() {
                if w.write_all(&zeros[..take]).is_err() {
                    return None;
                }
            }
            hasher.update(&zeros[..take]);
            left -= take as u64;
        }
        bytes = expected;
    }

    if bytes == 0 {
        return None;
    }
    if let Some(mut w) = out {
        if w.flush().is_err() {
            return None;
        }
    }
    Some(Written {
        bytes,
        sha256: format!("{:x}", hasher.finalize()),
        complete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("lazarbyte-recover-{}-{name}", std::process::id()))
    }

    fn pattern(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i % 251) as u8 ^ 0x5a).collect()
    }

    #[test]
    fn streams_across_chunk_boundaries_with_one_hash() {
        // Larger than one read chunk, so a bug in the chunk arithmetic or a
        // second hasher would show up as a different digest.
        let len = 20 * 1024 * 1024 + 1234;
        let body = pattern(len);
        let src_path = scratch("src.bin");
        let out_path = scratch("out.bin");
        std::fs::write(&src_path, &body).expect("write source");
        let src = Source::open(src_path.to_str().expect("path")).expect("open source");

        let written = copy(
            &src,
            &[Piece::Data(0, len as u64)],
            len as u64,
            Some(&out_path),
        )
        .expect("copy");

        assert_eq!(written.bytes, len as u64);
        assert!(written.complete);
        assert_eq!(written.sha256, format!("{:x}", Sha256::digest(&body)));
        assert_eq!(std::fs::read(&out_path).expect("read output"), body);
        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn a_hole_is_written_as_zeros_and_reported_incomplete() {
        let body = pattern(4096);
        let src_path = scratch("hole-src.bin");
        let out_path = scratch("hole-out.bin");
        std::fs::write(&src_path, &body).expect("write source");
        let src = Source::open(src_path.to_str().expect("path")).expect("open source");

        let pieces = [Piece::Data(0, 4096), Piece::Hole(1024)];
        let written = copy(&src, &pieces, 5120, Some(&out_path)).expect("copy");

        assert_eq!(written.bytes, 5120);
        assert!(!written.complete);
        let mut want = body.clone();
        want.resize(5120, 0);
        assert_eq!(written.sha256, format!("{:x}", Sha256::digest(&want)));
        assert_eq!(std::fs::read(&out_path).expect("read output"), want);
        let _ = std::fs::remove_file(&src_path);
        let _ = std::fs::remove_file(&out_path);
    }

    #[test]
    fn a_short_map_is_padded_to_the_declared_size() {
        let body = pattern(1000);
        let src_path = scratch("short-src.bin");
        std::fs::write(&src_path, &body).expect("write source");
        let src = Source::open(src_path.to_str().expect("path")).expect("open source");

        // A dry run hashes the padded content without writing anything.
        let written = copy(&src, &[Piece::Data(0, 1000)], 4000, None).expect("copy");

        assert_eq!(written.bytes, 4000);
        assert!(!written.complete);
        let mut want = body.clone();
        want.resize(4000, 0);
        assert_eq!(written.sha256, format!("{:x}", Sha256::digest(&want)));
        let _ = std::fs::remove_file(&src_path);
    }

    #[test]
    fn nothing_readable_yields_no_record() {
        let src_path = scratch("empty-src.bin");
        std::fs::write(&src_path, pattern(64)).expect("write source");
        let src = Source::open(src_path.to_str().expect("path")).expect("open source");

        // A data range entirely past the end reads nothing.
        assert!(copy(&src, &[Piece::Data(1 << 20, 4096)], 4096, None).is_none());
        let _ = std::fs::remove_file(&src_path);
    }
}
