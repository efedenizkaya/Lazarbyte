"""FFI parity harness: the desktop CLI vs the uniffi bridge, same fixture.

Faz 4's acceptance rule (AGENTS.md §6, §10): the same image scanned through
the CLI and through the FFI must produce the same manifest records, byte for
byte, apart from the fields that legitimately change per run. This script
checks that on the repository's fixture volumes, for all three engines the
bridge exports today.

What it does:
  1. builds the CLI and the cdylib (debug, unless --release);
  2. generates Python bindings for the built library;
  3. for each mode -- carve, FAT undelete, ext4 undelete -- runs the CLI with
     --machine, runs the same scan through Python, and compares every record
     on the fields that must be identical:
       name/ext, offset, size, sha256, validated, deleted (undelete only),
       and the output path relative to each run's output directory.
  4. normalizes the fields that legitimately differ: the output directory a
     record names is rewritten relative to that run's own out dir, so two
     runs writing into two directories still compare equal.

Fields NOT compared (per the §10 parity contract): wall-clock duration and
absolute paths outside the output directory -- each run writes to its own
directory, and the CLI records its own elapsed time.

Usage (from the repository root):
    python scripts/ffi-parity.py [--release] [--mode carve|fat|ext4 ...]

Exit status 0 means every compared field matched on every fixture.
"""

import argparse
import gzip
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
FIXTURES = REPO / "crates" / "lazarbyte-cli" / "tests" / "fixtures"


def build(profile: str) -> tuple[Path, Path]:
    """Build the CLI and the cdylib; return their paths."""
    # "debug" is the default profile; cargo has no --debug flag.
    cmd = ["cargo", "build"] if profile == "debug" else ["cargo", "build", "--release"]
    subprocess.run(cmd, cwd=REPO, check=True)
    exe = REPO / "target" / profile
    cli = exe / ("bcrumb-rs.exe" if os.name == "nt" else "bcrumb-rs")
    dll = exe / ("lazarbyte_ffi.dll" if os.name == "nt" else "liblazarbyte_ffi.so")
    assert cli.is_file(), cli
    assert dll.is_file(), dll
    return cli, dll


def make_bindings(dll: Path, out: Path) -> None:
    """Generate Python bindings for the built library and put the dll beside them."""
    subprocess.run(
        [
            "cargo", "run", "-q", "-p", "uniffi-bindgen", "generate",
            "--library", str(dll), "--language", "python", "--out-dir", str(out),
        ],
        cwd=REPO,
        check=True,
    )
    shutil.copy(dll, out / dll.name)


def unpack(name: str) -> Path:
    """Unpack a gzipped fixture into a temp file; return the image path."""
    data = gzip.open(FIXTURES / name, "rb").read()
    p = Path(tempfile.mkdtemp(prefix="ffi-parity-")) / "disk.dd"
    p.write_bytes(data)
    return p


def run_cli(cli: Path, out_dir: Path, img: Path, extra: list[str]) -> list[dict]:
    """Run the CLI in machine mode; return the JSON events it printed."""
    r = subprocess.run(
        [str(cli), "--machine", "--output", str(out_dir), "--min-free", "0", *extra, str(img)],
        capture_output=True,
        text=True,
    )
    if r.returncode != 0:
        sys.exit(f"CLI failed ({r.returncode}): {r.stderr[-2000:]}")
    return [json.loads(line) for line in r.stdout.splitlines() if line.strip()]


def rel(path: str, out_dir: Path) -> str:
    """A record's path, relative to its run's output directory, with / separators."""
    p = Path(path)
    if p.is_absolute():
        p = p.relative_to(out_dir)
    return p.as_posix()


def compare(label: str, cli_records: list[dict], ffi_records: list[dict], keys: list[str]) -> int:
    """Compare record lists on the given fields; return the number of problems."""
    problems = 0
    if len(cli_records) != len(ffi_records):
        print(f"FAIL {label}: {len(cli_records)} CLI records vs {len(ffi_records)} FFI records")
        problems += 1
    for i, (a, b) in enumerate(zip(cli_records, ffi_records)):
        for k in keys:
            va, vb = a.get(k), b.get(k)
            if va != vb:
                print(f"FAIL {label}[{i}] field {k!r}: CLI={va!r} FFI={vb!r}")
                problems += 1
    if problems == 0:
        print(f"ok   {label}: {len(cli_records)} records match on {keys}")
    return problems


def check_carve(cli: Path, bindings: Path) -> int:
    import lazarbyte_ffi as lb

    img = unpack("fat/exfat.dd.gz")
    cli_out = img.parent / "cli-out"
    ffi_out = img.parent / "ffi-out"

    # The CLI's defaults, mirrored on the FFI side; jobs=1 keeps record order
    # deterministic, which it already is on a single thread.
    events = run_cli(cli, cli_out, img, [])
    cli_records = [e for e in events if e.get("event") == "carve"]

    opts = lb.CarveOptions(
        out_dir=str(ffi_out), signature_types=[], start=0, length=0, min_size=0,
        max_size=0, chunk_size=32 << 20, align=1, dry_run=False, dedup=True,
        skip_blank=True, skip_carved=True, jobs=1, max_output=0, validate=False,
        drop_failed=False, max_validate=256 << 20,
    )
    src = lb.SourceHandle.from_path(str(img))
    ffi_records = [
        {
            "type": r.kind, "ext": r.ext, "offset": r.offset, "size": r.size,
            "sha256": r.sha256, "validated": r.validated, "decoded": r.decoded,
            "path": rel(r.path, ffi_out),
        }
        for r in lb.scan_carve(src, opts, None, None)
    ]
    for e in cli_records:
        e["path"] = rel(e["path"], cli_out)
    return compare("carve/exfat", cli_records, ffi_records,
                   ["type", "ext", "offset", "size", "sha256", "validated", "decoded", "path"])


def check_fat(cli: Path, bindings: Path) -> int:
    import lazarbyte_ffi as lb

    img = unpack("fat/exfat.dd.gz")
    cli_out = img.parent / "cli-out"
    ffi_out = img.parent / "ffi-out"

    events = run_cli(cli, cli_out, img, ["--fat"])
    cli_records = [e for e in events if e.get("event") == "file"]

    opts = lb.UndeleteOptions(out_dir=str(ffi_out), offset=0, include_live=False, min_size=0)
    result = lb.undelete_fat(lb.SourceHandle.from_path(str(img)), opts, None)
    ffi_records = [
        {
            "name": r.name, "offset": r.offset, "size": r.size, "sha256": r.sha256,
            "validated": r.validated, "deleted": r.deleted,
            "path": rel(r.path, ffi_out),
        }
        for r in result.records
    ]
    for e in cli_records:
        e["path"] = rel(e["path"], cli_out)
    n = compare("undelete/exfat", cli_records, ffi_records,
                ["name", "offset", "size", "sha256", "validated", "deleted", "path"])
    if result.kind != "exfat":
        print(f"FAIL undelete/exfat: volume kind {result.kind!r}, expected 'exfat'")
        n += 1
    return n


def check_ext4(cli: Path, bindings: Path) -> int:
    import lazarbyte_ffi as lb

    img = unpack("ext/ext4.img.gz")
    cli_out = img.parent / "cli-out"
    ffi_out = img.parent / "ffi-out"

    events = run_cli(cli, cli_out, img, ["--ext4"])
    cli_records = [e for e in events if e.get("event") == "file"]

    opts = lb.UndeleteOptions(out_dir=str(ffi_out), offset=0, include_live=False, min_size=0)
    result = lb.undelete_ext4(lb.SourceHandle.from_path(str(img)), opts, None)
    ffi_records = [
        {
            "name": r.name, "inode": r.inode, "size": r.size, "sha256": r.sha256,
            "validated": r.validated, "deleted": r.deleted,
            "path": rel(r.path, ffi_out),
        }
        for r in result.records
    ]
    for e in cli_records:
        e["path"] = rel(e["path"], cli_out)
    return compare("undelete/ext4", cli_records, ffi_records,
                   ["name", "inode", "size", "sha256", "validated", "deleted", "path"])


def main() -> int:
    ap = argparse.ArgumentParser(description="CLI vs FFI manifest parity on fixtures")
    ap.add_argument("--release", action="store_true", help="use the release profile")
    ap.add_argument("--mode", action="append", choices=["carve", "fat", "ext4"],
                    help="run only these modes (default: all)")
    args = ap.parse_args()

    profile = "release" if args.release else "debug"
    cli, dll = build(profile)
    bindings = Path(tempfile.mkdtemp(prefix="ffi-parity-bindings-"))
    make_bindings(dll, bindings)
    sys.path.insert(0, str(bindings))
    os.chdir(bindings)  # ctypes loads the dll from the bindings directory

    checks = {
        "carve": check_carve,
        "fat": check_fat,
        "ext4": check_ext4,
    }
    problems = 0
    for name, fn in checks.items():
        if args.mode and name not in args.mode:
            continue
        problems += fn(cli, bindings)
    print("PARITY OK" if problems == 0 else f"PARITY FAILED: {problems} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
