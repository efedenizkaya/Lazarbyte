//! Reading through a file the caller already holds.
//!
//! On Android the Storage Access Framework returns a document descriptor, not
//! a path, so the bridge wraps it in a `File` and `Source::from_file` takes it
//! from there. These tests use the same entry point with an ordinary file, so
//! the code path is exercised on every platform.

use lazarbyte_core::reader::Source;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("lazarbyte-from-file-{}-{name}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn a_file_handle_reads_like_a_path() {
    let p = scratch("body.bin");
    let body: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
    std::fs::write(&p, &body).expect("write");

    let by_path = Source::open(p.to_str().expect("path")).expect("open");
    let file = std::fs::File::open(&p).expect("open file");
    let by_file = Source::from_file(file, "saf:body.bin").expect("from file");

    assert_eq!(by_file.size(), body.len() as u64);
    assert_eq!(by_file.path(), "saf:body.bin");
    assert_eq!(by_file.pread(0, 4096), by_path.pread(0, 4096));
    assert_eq!(
        by_file.pread(body.len() as u64 - 10, 4096),
        body[body.len() - 10..].to_vec(),
        "a read past the end is short, not an error"
    );
    let _ = std::fs::remove_file(&p);
}

#[test]
fn an_empty_descriptor_is_refused() {
    let p = scratch("empty.bin");
    std::fs::write(&p, b"").expect("write");
    let file = std::fs::File::open(&p).expect("open file");
    let err = match Source::from_file(file, "saf:empty.bin") {
        Ok(_) => panic!("an empty descriptor was accepted"),
        Err(e) => e,
    };
    assert!(err.to_string().contains("cannot determine size"), "{err}");
    let _ = std::fs::remove_file(&p);
}
