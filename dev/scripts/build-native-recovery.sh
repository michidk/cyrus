#!/usr/bin/env bash
# Retain completed native compilation units and a validated executable across resets.
set -euo pipefail
cd "$(dirname "$0")/../.."
source dev/scripts/toolchain.sh
source dev/scripts/cargo.sh
: "${CARGO_TARGET_DIR:?preserve the inherited Cargo target directory}"
: "${CYRUS_NATIVE_CHECKPOINT_DIR:?set a host-mounted checkpoint directory}"
command -v hauler >/dev/null || { echo 'native recovery requires hauler' >&2; exit 1; }
CARGO_HAULER_MAX_CONCURRENT=1 CARGO_HAULER_OVERLAP_EXECUTION=0 hauler daemon start >/dev/null
output=$(realpath -m "${1:?usage: build-native-recovery.sh OUTPUT_EXECUTABLE}")
if [ "$(basename "$output")" != cyrus ]; then
    echo 'native executable output must be named cyrus' >&2
    exit 1
fi
case "$output" in
    "$(realpath -m "$CARGO_TARGET_DIR")"/*)
        echo 'retained executable output must be outside the shared Cargo target' >&2
        exit 1 ;;
esac
mkdir -p "$CYRUS_NATIVE_CHECKPOINT_DIR"
exec {checkpoint_lock}>"$CYRUS_NATIVE_CHECKPOINT_DIR/build.lock"
flock -n "$checkpoint_lock" || { echo 'native checkpoint build already running' >&2; exit 1; }
snapshot=$(mktemp)
current=$(mktemp)
trap 'rm -f "$snapshot" "$current"' EXIT
python3 dev/scripts/build-provenance.py snapshot "$snapshot"
configuration="package=cyrus profile=dev
$(rustc -vV)
$(cargo --version)
$(env | LC_ALL=C sort | rg '^(CARGO_PROFILE_|CARGO_BUILD_RUSTFLAGS=|CARGO_ENCODED_RUSTFLAGS=|RUSTFLAGS=|RUSTC=|RUSTC_WRAPPER=|CARGO_TARGET_DIR=|CARGO_HOME=|RUSTUP_HOME=)' || true)"
if python3 dev/scripts/web-build-checkpoint.py restore "$CYRUS_NATIVE_CHECKPOINT_DIR" \
    "$snapshot" "$configuration" "$output"; then
    echo "restored validated native executable: $output"
    exit
else
    status=$?
    if [ "$status" != 3 ]; then exit "$status"; fi
fi
python3 dev/scripts/web-compile-checkpoint.py restore "$CYRUS_NATIVE_CHECKPOINT_DIR" \
    "$snapshot" "$configuration" "$CARGO_TARGET_DIR"
cargo build -p cyrus --message-format=json-render-diagnostics | \
    python3 dev/scripts/web-compile-checkpoint.py record "$CYRUS_NATIVE_CHECKPOINT_DIR" \
        "$snapshot" "$configuration" "$CARGO_TARGET_DIR"
python3 dev/scripts/build-provenance.py snapshot "$current"
if ! cmp -s "$snapshot" "$current"; then
    echo 'source changed during native build; executable checkpoint rejected' >&2
    exit 1
fi
python3 dev/scripts/web-build-checkpoint.py store "$CYRUS_NATIVE_CHECKPOINT_DIR" \
    "$snapshot" "$configuration" "$CARGO_TARGET_DIR/debug/cyrus"
python3 dev/scripts/web-build-checkpoint.py restore "$CYRUS_NATIVE_CHECKPOINT_DIR" \
    "$snapshot" "$configuration" "$output"
echo "retained validated native executable: $output"
