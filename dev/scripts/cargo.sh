# shellcheck shell=bash
# Sourced by the build scripts: compile through the cargo-hauler daemon when it
# is installed (`just setup`), so concurrent agents share one build queue
# instead of blocking on Cargo's lock. Without it, plain cargo.
cargo() {
    if [ "${1:-}" = test ]; then
        # Unoptimized install parsers exceed libtest's 2 MiB worker stack.
        local RUST_MIN_STACK="${RUST_MIN_STACK:-8388608}"
        export RUST_MIN_STACK
    fi
    if [ -z "${CARGO_HAULER_INSIDE:-}" ] && command -v hauler >/dev/null 2>&1; then
        # Scripts need the final exit status and output, not an early detached ticket.
        (
            set -o pipefail
            # Opt-in Headstart: the daemon forwards this environment, so the patched
            # rustc and unstable flag reach a daemon started with
            # CARGO_HAULER_CARGO_BIN=<headstart cargo>. Native check/build only: its
            # rustc has no wasm std and clippy-driver is tied to the stock compiler.
            headstart="${VIBEPOD_HEADSTART_ROOT:-$HOME/workspaces/headstart}/rustc/build/host/stage1/bin"
            if [ "${CYRUS_HEADSTART:-}" = 1 ] && [ -x "$headstart/rustc" ] \
                && { [ "${1:-}" = check ] || [ "${1:-}" = build ]; } && [[ "$*" != *wasm32* ]]; then
                export RUSTC="$headstart/rustc" PATH="$headstart:$PATH" CARGO_UNSTABLE_HEADSTART=true
            fi
            CARGO_HAULER_MAX_CONCURRENT=3 CARGO_HAULER_SHIM_BACKGROUND=0 \
                hauler exec --host shim -- cargo "$@" | cat
        )
    else
        command cargo "$@"
    fi
}
