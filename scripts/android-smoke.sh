#!/usr/bin/env bash
# Android smoke test: run the bridge's smoke binary on a device or emulator.
#
# Prerequisites: cargo-ndk, an Android SDK with adb on PATH, and a connected
# device or running emulator (start one with the SDK's emulator command).
#
# Usage: scripts/android-smoke.sh <fixture.gz> [fat|carve] [serial]
#
# The script unpacks the fixture, pushes the x86_64 binary and the image to
# /data/local/tmp on the device, runs the scan there, and prints the output.
# Compare it with the desktop run to check parity; the exit status is the
# binary's.
set -euo pipefail

# Git Bash (MSYS) rewrites arguments starting with / into Windows paths; the
# device-side paths below must reach adb untouched. Harmless elsewhere.
export MSYS_NO_PATHCONV=1
export MSYS2_ARG_CONV_EXCL="*"

# With conversion off, local Windows paths must be converted by hand.
if command -v cygpath >/dev/null 2>&1; then
    winpath() { cygpath -w "$1"; }
else
    winpath() { printf '%s' "$1"; }
fi

REPO="$(cd "$(dirname "$0")/.." && pwd)"
FIXTURE="$1"
MODE="${2:-carve}"
SERIAL="${3:-}"

ABI="${ANDROID_SMOKE_ABI:-x86_64}"   # the emulator's ABI; arm64-v8a needs a real device
TMP="$(mktemp -d)"

case "$ABI" in
    arm64-v8a) TARGET="aarch64-linux-android" ;;
    x86_64) TARGET="x86_64-linux-android" ;;
    *) TARGET="$ABI-linux-android" ;;
esac

echo "== building android_smoke for $ABI (profile android)"
(cd "$REPO" && cargo ndk --platform 21 -t "$ABI" build --profile android -p lazarbyte-ffi --bins)
BIN="$REPO/target/$TARGET/android/android_smoke"

gzip -dc "$FIXTURE" > "$TMP/disk.dd"

ADB="${ANDROID_HOME:-$LOCALAPPDATA/Android/Sdk}/platform-tools/adb.exe"
[ -x "$ADB" ] || ADB=adb
ADB_ARGS=()
[ -n "$SERIAL" ] && ADB_ARGS=(-s "$SERIAL")
"$ADB" "${ADB_ARGS[@]}" wait-for-device

echo "== pushing binary and image to /data/local/tmp"
"$ADB" "${ADB_ARGS[@]}" push "$(winpath "$BIN")" /data/local/tmp/android_smoke >/dev/null
"$ADB" "${ADB_ARGS[@]}" push "$(winpath "$TMP/disk.dd")" /data/local/tmp/disk.dd >/dev/null
"$ADB" "${ADB_ARGS[@]}" shell "chmod +x /data/local/tmp/android_smoke"

echo "== running on device"
"$ADB" "${ADB_ARGS[@]}" shell "rm -rf /data/local/tmp/out && /data/local/tmp/android_smoke /data/local/tmp/disk.dd /data/local/tmp/out $MODE"

echo "== cleaning up device"
"$ADB" "${ADB_ARGS[@]}" shell "rm -rf /data/local/tmp/android_smoke /data/local/tmp/disk.dd /data/local/tmp/out"
rm -rf "$TMP"
