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
recorder_pid=
cleanup() {
    if [ -n "$recorder_pid" ]; then
        kill "$recorder_pid" 2>/dev/null || true
        wait "$recorder_pid" 2>/dev/null || true
    fi
    rm -f "$snapshot" "$current"
}
trap cleanup EXIT
python3 dev/scripts/build-provenance.py snapshot "$snapshot"
configuration="package=cyrus profile=dev
$(rustc -vV)
$(cargo --version)
$(env | LC_ALL=C sort | rg '^(CARGO_PROFILE_|CARGO_BUILD_RUSTFLAGS=|CARGO_ENCODED_RUSTFLAGS=|RUSTFLAGS=|RUSTC=|RUSTC_WRAPPER=|CARGO_TARGET_DIR=|CARGO_HOME=|RUSTUP_HOME=)' || true)"
if python3 dev/scripts/web-build-checkpoint.py restore "$CYRUS_NATIVE_CHECKPOINT_DIR" \
    "$snapshot" "$configuration" "$output"; then
    python3 dev/scripts/build-provenance.py seal "$snapshot" "$output"
    mv "$snapshot" "$output.provenance.json"
    echo "restored validated native executable: $output"
    exit
else
    status=$?
    if [ "$status" != 3 ]; then exit "$status"; fi
fi
python3 dev/scripts/web-compile-checkpoint.py restore "$CYRUS_NATIVE_CHECKPOINT_DIR" \
    "$snapshot" "$configuration" "$CARGO_TARGET_DIR"
# Spool Cargo events on the host so unit compression cannot backpressure Cargo
# or defer sealing an already completed executable.
events="$CYRUS_NATIVE_CHECKPOINT_DIR/cargo-events.jsonl"
: > "$events"
cargo build -p cyrus --message-format=json-render-diagnostics > "$events" &
build_pid=$!
tail --pid="$build_pid" -n +1 -f "$events" | \
    python3 dev/scripts/web-compile-checkpoint.py record "$CYRUS_NATIVE_CHECKPOINT_DIR" \
        "$snapshot" "$configuration" "$CARGO_TARGET_DIR" &
recorder_pid=$!
status=0
wait "$build_pid" || status=$?
if [ "$status" != 0 ]; then
    wait "$recorder_pid" || true
    recorder_pid=
    exit "$status"
fi
python3 dev/scripts/build-provenance.py snapshot "$current"
if ! cmp -s "$snapshot" "$current"; then
    echo 'source changed during native build; executable checkpoint rejected' >&2
    exit 1
fi
python3 dev/scripts/web-build-checkpoint.py store "$CYRUS_NATIVE_CHECKPOINT_DIR" \
    "$snapshot" "$configuration" "$CARGO_TARGET_DIR/debug/cyrus"
python3 dev/scripts/web-build-checkpoint.py restore "$CYRUS_NATIVE_CHECKPOINT_DIR" \
    "$snapshot" "$configuration" "$output"
cp "$snapshot" "$current"
python3 dev/scripts/build-provenance.py seal "$current" "$output"
mv "$current" "$output.provenance.json"
echo "retained validated native executable: $output"
wait "$recorder_pid"
recorder_pid=
