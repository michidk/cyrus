#!/usr/bin/env bash
# Build a retained capture executable without changing the shared Cargo target.
set -euo pipefail
cd "$(dirname "$0")/../.."
output=$(realpath -m "${CYRUS_NATIVE_EXECUTABLE:-magic/build-tools/native/cyrus}")
export CYRUS_NATIVE_CHECKPOINT_DIR="${CYRUS_NATIVE_CHECKPOINT_DIR:-$(dirname "$output")}"
bash dev/scripts/build-native-recovery.sh "$output"
