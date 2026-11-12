# Build profiles

`lazarbyte-core` compiles in two shapes.

**Everything** (default) is what the desktop CLI, the test suite and the parity
harness use. It matches upstream breadcrumb-rs byte for byte.

**Mobile** is `--no-default-features`: carving, FAT/exFAT and ext. This is the
scope the Android app promises, and the build the FFI crate will use.

| Feature | Adds | Default |
| --- | --- | --- |
| `ntfs` | NTFS undelete; implies `artifacts` | on |
| `artifacts` | `$Recycle.Bin` / `$UsnJrnl` report | on |
| `hfs` | HFS+/HFSX undelete | on |
| `apfs` | APFS recovery | on |
| `ewf` | EWF/E01 images, `--verify` against stored hashes | on |
| `bitlocker` | BitLocker volumes (and the `aes` dependency) | on |
| `containers` | QCOW2, VMDK, split raw, stdin | on |
| `grep` | `--grep` (and the `regex` dependency) | on |
| `customsig` | `--sig-file` (and `jsonin`) | on |

What neither profile can drop: carving itself, the signature registry, chunked
scan and output budget, the source reader, FAT/exFAT and ext. A feature only
removes code; turning one on never changes a result.

```sh
# Desktop, everything
cargo build --release

# Android profile
cargo build --release -p lazarbyte-core --no-default-features
cargo clippy -p lazarbyte-core --no-default-features -- -D warnings
```

## Sizes measured on Windows x86_64 (release, `lto = true`)

| Build | Compiled crate (rlib) | Build time |
| --- | --- | --- |
| Everything | 3110 KiB | 11.5 s |
| Mobile | 1521 KiB | 5.8 s |

The desktop CLI binary is 1.9 MiB of which 1.6 MiB is `.text`; the `regex`
family alone is 447 KiB of that, so `grep` is the single largest saving. These
are compiled-artifact numbers, not APK numbers: what the linker keeps after
LTO decides the final package, and that is measured in Faz 5 on arm64.
