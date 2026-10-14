#!/usr/bin/env bash
# External consumers exclude workspace/dev feature unification from boundary checks.
set -euo pipefail
cd "$(dirname "$0")/../.."
source dev/scripts/toolchain.sh
. dev/scripts/cargo.sh
root=$PWD
export CARGO_TARGET_DIR="$root/target/boundary-build"

check_consumer() {
    local name=$1 forbidden=$2 exports=$3
    shift 3
    local probe="$root/target/boundary-consumer"
    mkdir -p "$probe/src"
    cp Cargo.lock "$probe/Cargo.lock"
    cat > "$probe/Cargo.toml" <<MANIFEST
[package]
name = "cyrus-$name-boundary-check"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
MANIFEST
    for dependency in "$@"; do
        echo "$dependency = { path = \"$root/crates/$dependency\" }" >> "$probe/Cargo.toml"
    done
    printf '%s\n' "$exports" > "$probe/src/lib.rs"
    cargo tree --manifest-path "$probe/Cargo.toml" --edges normal,build --prefix none > "$probe/tree"
    if grep -E "$forbidden" "$probe/tree"; then
        echo "$name boundary: forbidden production dependency; restore the documented crate ownership" >&2
        exit 1
    fi
    cargo check --manifest-path "$probe/Cargo.toml"
    echo "$name boundary: isolated consumer passed"
}

check_consumer script '^(bevy(_[^ ]+)?|cyrus-assets|redguard-preservation) ' \
    'pub use cyrus_script::{Runtime, program::Program};' cyrus-script

check_consumer core '^(bevy_(render|window|winit|audio|pbr|gltf|mesh|image)|redguard-preservation|cyrus-(render|control)|wgpu|winit|cpal|rodio) ' \
    'pub use cyrus_assets::animation::Animations;
pub use cyrus_assets::sound::{DialogueCache, DialogueEntry, DialoguePayload};
pub fn dialogue_cache() -> DialogueCache<DialogueEntry> { DialogueCache::default() }
pub use cyrus_collision::collision_world::StaticCollision;
pub use cyrus_media::{Movie, MovieStream, Packets};
pub use cyrus_world::{animation::AnimationPlugin, lifecycle::WorldPlugin};
pub use cyrus_game::GamePlugin;
pub use cyrus_console::{ConsoleApp, ConsolePlugin};
pub use cyrus_script::Runtime;' \
    cyrus-assets cyrus-media cyrus-world cyrus-collision cyrus-game cyrus-console cyrus-script

check_consumer app '^cyrus-(game|world|collision|audio|render|startup|debug|viewer|control|mcp|web) ' \
    'pub use cyrus_app::{EguiPlugin, launch::InstallArgs, loading::LoadingOverlayPlugin};' cyrus-app

check_consumer render '^cyrus-(audio|startup|app|debug) ' \
    'pub use cyrus_render::WorldRenderPlugin;' cyrus-render
