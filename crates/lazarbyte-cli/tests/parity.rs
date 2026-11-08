//! Old-binary vs new-binary parity harness.
//!
//! Faz 1 must not change behaviour, so the pre-split binary and the split
//! workspace binary have to produce the same output for the same input. This
//! test compares both on the FAT16 fixture in carve mode and in `--fat`
//! undelete mode.
//!
//! It is ignored by default because the repository does not carry the old
//! binary. Build it from the commit before the split, then run:
//!
//! ```text
//! $env:BCRUMB_OLD_BIN = "C:\path\to\old\bcrumb-rs.exe"
//! cargo test -p lazarbyte-cli --test parity -- --ignored
//! ```
//!
//! Two kinds of field legitimately differ between runs and are normalised
//! before the comparison, so they are not false positives:
//!
//! * `elapsed_s` -- wall-clock scan duration;
//! * `path` / `source` under the output directory -- each run writes to its
//!   own directory. The input image path is identical for both runs and is
//!   therefore compared as-is.

use flate2::read::GzDecoder;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let mut p = std::env::temp_dir();
        p.push(format!("bcrumb-parity-{tag}-{}", std::process::id()));
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

/// Replace the wall-clock duration and the run-specific output directory with
/// placeholders, so only behaviour-bearing differences remain.
fn normalize(text: &str, out: &Path) -> String {
    let raw = out.to_string_lossy().to_string();
    let escaped = raw.replace('\\', "\\\\");
    let mut s = text.replace(&escaped, "OUT").replace(&raw, "OUT");
    const KEY: &str = "\"elapsed_s\": ";
    let mut from = 0;
    while let Some(i) = s[from..].find(KEY) {
        let start = from + i + KEY.len();
        let mut end = start;
        let bytes = s.as_bytes();
        while end < bytes.len() && !matches!(bytes[end], b',' | b'}') {
            end += 1;
        }
        s.replace_range(start..end, "0");
        from = start + 1;
    }
    s
}

/// Every regular file below `dir`, as (forward-slash relative path, contents).
fn files(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                let rel = p
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn run(bin: &Path, img: &Path, out: &Path, args: &[&str]) {
    let status = Command::new(bin)
        .arg(img)
        .arg("-o")
        .arg(out)
        .args(args)
        .arg("-q")
        .status()
        .unwrap_or_else(|e| panic!("cannot run {}: {e}", bin.display()));
    assert!(status.success(), "{} exited with {status}", bin.display());
}

fn compare(mode: &str, old_out: &Path, new_out: &Path) {
    let manifest = |dir: &Path| -> String {
        normalize(
            &std::fs::read_to_string(dir.join("manifest.json")).unwrap(),
            dir,
        )
    };
    assert_eq!(
        manifest(old_out),
        manifest(new_out),
        "{mode}: manifest.json differs"
    );

    let old_files = files(old_out);
    let new_files = files(new_out);
    let names =
        |v: &[(String, Vec<u8>)]| -> Vec<String> { v.iter().map(|(n, _)| n.clone()).collect() };
    assert_eq!(
        names(&old_files),
        names(&new_files),
        "{mode}: file list differs"
    );
    for ((name, a), (_, b)) in old_files.iter().zip(&new_files) {
        if name.ends_with(".json") || name.ends_with(".jsonl") {
            assert_eq!(
                normalize(&String::from_utf8_lossy(a), old_out),
                normalize(&String::from_utf8_lossy(b), new_out),
                "{mode}: {name} differs"
            );
        } else {
            assert_eq!(a, b, "{mode}: {name} differs");
        }
    }
}

#[test]
#[ignore = "requires BCRUMB_OLD_BIN pointing at a pre-split bcrumb-rs binary"]
fn old_and_new_binaries_agree() {
    let old = PathBuf::from(
        std::env::var_os("BCRUMB_OLD_BIN")
            .expect("set BCRUMB_OLD_BIN to the pre-split binary before running this test"),
    );
    let new = PathBuf::from(env!("CARGO_BIN_EXE_bcrumb-rs"));

    let dir = Tmp::new("fat16");
    let gz = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fat/fat16.dd.gz");
    let mut image = Vec::new();
    GzDecoder::new(std::fs::File::open(&gz).unwrap())
        .read_to_end(&mut image)
        .expect("fixture did not decompress");
    let img = dir.0.join("fat16.dd");
    std::fs::write(&img, &image).unwrap();

    let modes: &[(&str, &[&str])] = &[("carve", &["--chunk", "1M"]), ("fat", &["--fat"])];
    for (mode, args) in modes {
        let old_out = dir.0.join(format!("old-{mode}"));
        let new_out = dir.0.join(format!("new-{mode}"));
        run(&old, &img, &old_out, args);
        run(&new, &img, &new_out, args);
        compare(mode, &old_out, &new_out);
    }
}
