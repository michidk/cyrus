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
tools="$root/magic/build-tools"
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
    rustup target add wasm32-unknown-unknown wasm32-wasip2 # browser build, native mods
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
if [ "${BASH_SOURCE[0]}" != "$0" ]; then
    return 0
fi
exec bash -euo pipefail "$@"
