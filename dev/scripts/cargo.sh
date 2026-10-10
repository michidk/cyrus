# shellcheck shell=bash
# Sourced by the build scripts: compile through the cargo-hauler daemon when it
# is installed (`just setup`), so concurrent agents share one build queue
# instead of blocking on Cargo's lock. Without it, plain cargo.
cargo() {
    if [ -z "${CARGO_HAULER_INSIDE:-}" ] && command -v hauler >/dev/null 2>&1; then
        hauler exec --host script -- cargo "$@"
    else
        command cargo "$@"
    fi
}
