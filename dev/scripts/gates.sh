#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
source dev/scripts/toolchain.sh
. dev/scripts/cargo.sh
# Cargo tests run from crate directories; make the workspace link explicit.
if [ -z "${REDGUARD_DIR+x}" ] && [ -d redguard ]; then
    export REDGUARD_DIR="$(cd redguard && pwd -P)"
fi
dev/scripts/publish-check.sh
dev/scripts/genmap.sh --check
python3 dev/scripts/test-build-provenance.py
node --test dev/scripts/capture-provenance.test.mjs dev/scripts/browser-provenance.test.mjs dev/scripts/matched-capture.test.mjs
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
log=$(mktemp)
trap 'rm -f "$log"' EXIT
cargo test --workspace -- --nocapture 2>&1 | tee "$log"
# Tests that need the install say "skipped: no install". With an install linked,
# a skip hides a broken loader behind a green run; without one, it is expected.
skipped=$(grep -c 'skipped: no install' "$log" || true)
if [ -e redguard/RGFX.EXE ] && [ "$skipped" != 0 ]; then
    echo "gates: $skipped install tests skipped although redguard/ is an install" >&2
    exit 1
fi
echo "gates: $skipped install tests skipped"
echo "gates: green"
