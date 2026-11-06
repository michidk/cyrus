#!/usr/bin/env bash
# Rootless Rust toolchain for containers without sudo, kept under
# magic/build-tools (untracked): rustup, the wasm target, wasm-bindgen and
# Bevy's Linux libraries unpacked from Debian packages.
#
#   dev/scripts/toolchain.sh install   # `just toolchain`: install or update it
#   source dev/scripts/toolchain.sh    # bash only: export it when cargo is not on PATH
#   set shell := ["dev/scripts/toolchain.sh", "-c"]   # the justfile's shell
#
# A system-wide cargo on PATH always wins; this only fills the gap.
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
tools="$(realpath -m "$root/magic/build-tools")"
sysdeps="$tools/sysdeps"
arch_lib="usr/lib/x86_64-linux-gnu"

use_tools() {
    export CARGO_HOME="$tools/cargo" RUSTUP_HOME="$tools/rustup"
    export PATH="$tools/cargo/bin:$PATH"
    export PKG_CONFIG_PATH="$sysdeps/$arch_lib/pkgconfig" PKG_CONFIG_SYSROOT_DIR="$sysdeps"
    export LD_LIBRARY_PATH="$sysdeps/$arch_lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
}

install() {
    set -euo pipefail
    cd "$root"
    mkdir -p "$tools"
    use_tools
    if [ ! -x "$tools/cargo/bin/rustup" ]; then
        curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain none
    fi
    rustup show active-toolchain >/dev/null # installs rust-toolchain.toml's channel and components
    rustup target add wasm32-unknown-unknown
    wb_version="$(awk '/^name = "wasm-bindgen"$/ { getline; sub(/version = /, ""); gsub(/"/, ""); print }' Cargo.lock)"
    if ! wasm-bindgen --version 2>/dev/null | grep -q " $wb_version$"; then
        cargo install wasm-bindgen-cli --version "$wb_version" --locked
    fi
    if [ ! -d "$sysdeps" ]; then
        # Bevy's Linux build dependencies; apt-get download works without root.
        apt="$tools/apt"
        mkdir -p "$apt/lists/partial" "$apt/cache/archives/partial" "$tools/debs"
        opts=(-q -o "Dir::State::Lists=$apt/lists" -o "Dir::Cache=$apt/cache")
        apt-get "${opts[@]}" update >"$apt/update.log"
        (cd "$tools/debs" && apt-get "${opts[@]}" download \
            libasound2 libasound2-dev libudev1 libudev-dev libffi-dev \
            libwayland-client0 libwayland-cursor0 libwayland-egl1 libwayland-server0 libwayland-dev \
            libxkbcommon0 libxkbcommon-dev)
        mkdir -p "$sysdeps"
        for deb in "$tools"/debs/*.deb; do dpkg -x "$deb" "$sysdeps"; done
    fi
    echo "toolchain: $(cargo --version), wasm-bindgen $wb_version, libraries in $sysdeps"
}

if [ "${1:-}" = install ]; then
    install
    exit
fi
if ! command -v cargo >/dev/null && [ -x "$tools/cargo/bin/cargo" ]; then
    use_tools
fi

# Registry and git dependencies compile from files under CARGO_HOME. Checkouts with their own
# CARGO_HOME (a toolchain copy, or a symlinked tools directory) would otherwise write the same
# artifact names into a shared CARGO_TARGET_DIR with a crate hash that depends on that path, which
# strands every dependent built against the other copy (rustdoc: E0463 can't find crate).
# Remapping the dependency directories makes the hash path-independent.
if [ -n "${CARGO_HOME:-}" ]; then
    export CARGO_HOME="$(realpath -m "$CARGO_HOME")"
    if [ -z "${RUSTFLAGS:-}" ] && [ -z "${CARGO_ENCODED_RUSTFLAGS:-}" ]; then
        for dependency_dir in registry/src git/checkouts; do
            dependency_remap="--remap-path-prefix=$CARGO_HOME/$dependency_dir=/cargo/$dependency_dir"
            case " ${CARGO_BUILD_RUSTFLAGS:-} " in
                *" $dependency_remap "*) ;;
                *) CARGO_BUILD_RUSTFLAGS="${CARGO_BUILD_RUSTFLAGS:+$CARGO_BUILD_RUSTFLAGS }$dependency_remap" ;;
            esac
        done
        export CARGO_BUILD_RUSTFLAGS
    fi
fi
if [ "${BASH_SOURCE[0]}" != "$0" ]; then
    return 0
fi
exec bash -euo pipefail "$@"
