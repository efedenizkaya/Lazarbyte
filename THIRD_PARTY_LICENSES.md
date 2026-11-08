# Third-party licenses

Lazarbyte itself is licensed under the Apache License, Version 2.0 (see
`LICENSE`). This file records the origin and licensing of everything that came
from elsewhere.

## Upstream project: breadcrumb-rs

Lazarbyte is a fork of [sltcnb/breadcrumb-rs](https://github.com/sltcnb/breadcrumb-rs),
which is licensed under the MIT License. The source files carried over from the
fork are used and modified under that license. The original license text is
kept verbatim in `LICENSE-MIT` and reproduced below.

```
MIT License

Copyright (c) 2026 BreadCrumb contributors

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Rust dependencies

Versions are those pinned in `Cargo.lock` at the time of writing. All of them
are permissive licenses from the project's allow-list (MIT, Apache-2.0,
BSD/0BSD, ISC, Unlicense, Zlib).

| Crate | Version | License | Source |
| --- | --- | --- | --- |
| adler2 | 2.0.1 | 0BSD OR MIT OR Apache-2.0 | https://github.com/oyvindln/adler2 |
| aes | 0.8.4 | MIT OR Apache-2.0 | https://github.com/RustCrypto/block-ciphers |
| aho-corasick | 1.1.5 | Unlicense OR MIT | https://github.com/BurntSushi/aho-corasick |
| block-buffer | 0.10.4 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| cfg-if | 1.0.4 | MIT OR Apache-2.0 | https://github.com/rust-lang/cfg-if |
| cipher | 0.4.4 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| cpufeatures | 0.2.17 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| crc32fast | 1.5.0 | MIT OR Apache-2.0 | https://github.com/srijs/rust-crc32fast |
| crypto-common | 0.1.7 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| digest | 0.10.7 | MIT OR Apache-2.0 | https://github.com/RustCrypto/traits |
| flate2 | 1.1.9 | MIT OR Apache-2.0 | https://github.com/rust-lang/flate2-rs |
| generic-array | 0.14.7 | MIT | https://github.com/fizyk20/generic-array |
| inout | 0.1.4 | MIT OR Apache-2.0 | https://github.com/RustCrypto/utils |
| libc | 0.2.189 | MIT OR Apache-2.0 | https://github.com/rust-lang/libc |
| md-5 | 0.10.6 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| memchr | 2.8.3 | Unlicense OR MIT | https://github.com/BurntSushi/memchr |
| miniz_oxide | 0.8.9 | MIT OR Zlib OR Apache-2.0 | https://github.com/Frommi/miniz_oxide |
| regex | 1.13.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| regex-automata | 0.4.18 | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| regex-syntax | 0.8.11 | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| sha1 | 0.10.7 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| simd-adler32 | 0.3.10 | MIT | https://github.com/mcountryman/simd-adler32 |
| typenum | 1.20.1 | MIT OR Apache-2.0 | https://github.com/paholg/typenum |
| version_check | 0.9.5 | MIT OR Apache-2.0 | https://github.com/SergioBenitez/version_check |

Full license texts for these crates are shipped inside the crates themselves
(`~/.cargo/registry/src/...`). They must be collected into the packaged app's
third-party notices before any Play Store release; that is a Faz 8 task, not
done yet.

## Fuzz-only development crates

`fuzz/` is a workspace of its own, used only by `cargo fuzz`. Nothing in it is
linked into the CLI or the Android app, and none of it is distributed. CI keeps
its lockfile consistent (`cargo metadata --locked`), but the crates are outside
`cargo deny check` because two of them use licenses that are permissive yet not
on the project allow-list:

| Crate | Version | License | Source |
| --- | --- | --- | --- |
| libfuzzer-sys | 0.4.13 | (MIT OR Apache-2.0) AND NCSA | https://github.com/rust-fuzz/libfuzzer |
| arbitrary | 1.4.2 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/rust-fuzz/arbitrary |
| derive_arbitrary | 1.4.2 | (MIT OR Apache-2.0) AND Unicode-3.0 | https://github.com/rust-fuzz/arbitrary |

Both NCSA and Unicode-3.0 are OSI-approved and permissive. Bringing the fuzz
workspace under the same `cargo deny` gate therefore needs the project owner's
explicit approval; until then this is recorded rather than silently allowed.

## Enforcement

`deny.toml` makes [cargo-deny](https://github.com/embarkstudios/cargo-deny)
enforce this file's rules: the permissive allow-list above, the scoped
uniffi MPL-2.0 exception, crates.io as the only allowed source, and a ban on
OpenSSL/native-tls so the core stays pure Rust and offline. Run
`cargo deny check` after every dependency change. Until Faz 4 adds uniffi,
cargo-deny reports the uniffi exceptions as "not encountered"; that warning is
expected.

## Planned: uniffi (Faz 4)

The Android/Kotlin bridge will use the [uniffi-rs](https://github.com/mozilla/uniffi-rs)
family, which is licensed under the **MPL-2.0**. This is the only exception to
the project's permissive-license allow-list, and it is limited to unmodified
uniffi crates used as dependencies (see `AGENTS.md` §2 and `deny.toml`).

- uniffi, uniffi_core, uniffi_macros, uniffi_meta, uniffi_build,
  uniffi_bindgen, uniffi_udl — MPL-2.0 — https://github.com/mozilla/uniffi-rs
- uniffi source files are never vendored, patched or modified in this
  repository. Using them as a dependency does not relicense Lazarbyte.

Full MPL-2.0 text: https://www.mozilla.org/en-US/MPL/2.0/
