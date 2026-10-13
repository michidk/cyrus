#!/usr/bin/env bash
# Build the capture executable and its provenance without launching a game.
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/scripts/toolchain.sh
. dev/scripts/cargo.sh
export CARGO_TARGET_DIR="$PWD/target"
provenance=$(mktemp)
trap 'rm -f "$provenance"' EXIT
python3 dev/scripts/build-provenance.py snapshot "$provenance"
rm -f target/debug/cyrus.provenance.json
cargo build -p cyrus
python3 dev/scripts/build-provenance.py seal "$provenance" target/debug/cyrus
mv "$provenance" target/debug/cyrus.provenance.json
