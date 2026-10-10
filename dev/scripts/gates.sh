#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/scripts/cargo.sh
dev/scripts/publish-check.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
echo "gates: green"
