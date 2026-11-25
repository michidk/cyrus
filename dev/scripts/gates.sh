#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/../.."
stage="${1:-all}"
case "$stage" in
    all|check|verify|lints|test|packaging-macos) ;;
    *) echo "usage: gates.sh [all|check|verify|lints|test|packaging-macos] [package]" >&2; exit 2 ;;
esac
# This focused platform contract also runs under the system macOS Bash.
if [[ "$stage" == packaging-macos ]]; then
    python3 -B dev/scripts/test-build-artifacts.py \
        ArtifactTest.test_native_platforms_and_optional_viewer \
        ArtifactTest.test_mod_bundle_delivers_launcher_beside_game_on_each_platform \
        ArtifactTest.test_raytracing_packages_preserve_standard_downloads \
        ArtifactTest.test_build_failure_produces_no_archive
    exit 0
fi
source dev/scripts/toolchain.sh
. dev/scripts/cargo.sh
# Cargo tests run from crate directories; make the workspace link explicit.
if [ -z "${REDGUARD_DIR+x}" ] && [ -d redguard ]; then
    export REDGUARD_DIR="$(cd redguard && pwd -P)"
fi
if [[ "$stage" == all || "$stage" == check || "$stage" == lints ]]; then
    for test in dev/scripts/test-*.py; do
        echo "gates: $test"
        python3 -B "$test"
    done
    node --test dev/scripts/*.test.mjs dev/scripts/test-local-install.mjs dev/scripts/test-game-assets.mjs
    python3 -B dev/scripts/check-preservation-release.py
fi
if [[ "$stage" == check ]]; then
    dev/scripts/publish-check.sh
    dev/scripts/genmap.sh --check
    cargo fmt --all -- --check
    exit 0
fi
if [[ "$stage" == verify ]]; then
    package="${2:-}"
    [[ -n "$package" ]] || { echo "usage: gates.sh verify <package>" >&2; exit 2; }
    case "$package" in
        cyrus|cyrus-*) ;;
        *) package="cyrus-$package" ;;
    esac
    cargo clippy -p "$package" --all-targets -- -D warnings
fi
if [[ "$stage" != test && "$stage" != verify ]]; then
    dev/scripts/publish-check.sh
    dev/scripts/genmap.sh --check
    cargo fmt --all -- --check
    dev/scripts/check-core-boundaries.sh
    cargo clippy --workspace --all-targets -- -D warnings
fi
if [[ "$stage" == lints ]]; then exit 0; fi
log=$(mktemp)
trap 'rm -f "$log"' EXIT
if [[ "$stage" == verify ]]; then
    cargo test -p "$package" -- --nocapture 2>&1 | tee "$log"
elif [[ -n "${CYRUS_TEST_MESSAGES:-}" ]]; then
    cargo test --workspace --all-features --message-format=json 2>&1 | tee "$CYRUS_TEST_MESSAGES" "$log"
else
    cargo test --workspace -- --nocapture 2>&1 | tee "$log"
fi
# Tests that need the install say "skipped: no install". With an install linked,
# a skip hides a broken loader behind a green run; without one, it is expected.
skipped=$(grep -c 'skipped: no install' "$log" || true)
if [ -e redguard/RGFX.EXE ] && [ "$skipped" != 0 ]; then
    echo "gates: $skipped install tests skipped although redguard/ is an install" >&2
    exit 1
fi
echo "gates: $skipped install tests skipped"
echo "gates: green"
