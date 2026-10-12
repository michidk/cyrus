#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
source dev/scripts/toolchain.sh
. dev/scripts/cargo.sh
dev/scripts/publish-check.sh
dev/scripts/genmap.sh --check
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
log=$(mktemp)
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
