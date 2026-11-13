//! A smoke-test runner for the bridge on an Android device or emulator.
//!
//! This is development tooling, not a product: cargo-ndk builds it, the
//! emulator smoke test pushes it over adb and runs it on a fixture image, and
//! its output is compared with the desktop result. It exercises the core on
//! Android -- opening a source, scanning, writing through the default sink --
//! plus one call into the bridge crate to prove the whole library links.
//! The uniffi C ABI itself is tested on desktop (scripts/ffi-parity.py); the
//! app exercises it on device from Faz 6.
//!
//! Usage: android_smoke <image> <out-dir> [fat|carve]
//! Prints one line per record, then a final SUMMARY line.

use lazarbyte_core::carver::{self, Options};
use lazarbyte_core::reader::Source;
use lazarbyte_core::signatures::SIGNATURES;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: android_smoke <image> <out-dir> [fat|carve]");
        std::process::exit(2);
    }
    let img = &args[1];
    let out_dir = &args[2];
    let mode = args.get(3).map(String::as_str).unwrap_or("carve");

    // Proof the bridge crate links and its export surface was generated.
    println!("bridge_version {}", lazarbyte_ffi::bridge_version());

    let src = match Source::open(img) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("open failed: {e}");
            std::process::exit(1);
        }
    };

    match mode {
        "fat" => {
            // fat::Options has no Default; spell the defaults out.
            let opts = lazarbyte_core::fat::Options {
                out_dir: out_dir.clone(),
                dry_run: false,
                include_live: false,
                min_size: 0,
                cancel: None,
                sink: None,
            };
            let (records, kind, _cluster) =
                lazarbyte_core::fat::recover(&src, 0, &opts, |_| {}).expect("fat recover");
            for r in &records {
                println!("fat {} {} {} {}", r.name, r.offset, r.size, r.sha256);
            }
            println!("SUMMARY fat {kind} {} records", records.len());
        }
        _ => {
            let opts = Options {
                out_dir: out_dir.clone(),
                jobs: 1,
                ..Options::default()
            };
            let records = carver::run_parallel(&src, &SIGNATURES.iter().collect::<Vec<_>>(), &opts)
                .expect("carve scan");
            for r in &records {
                println!("carve {} {} {} {}", r.kind, r.offset, r.size, r.sha256);
            }
            println!("SUMMARY carve {} records", records.len());
        }
    }
}
