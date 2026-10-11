#!/usr/bin/env bash
# An external consumer excludes workspace/dev feature unification from the core check.
set -euo pipefail
cd "$(dirname "$0")/../.."
source dev/scripts/toolchain.sh
. dev/scripts/cargo.sh
root=$PWD
probe="$root/target/core-boundary-consumer"
mkdir -p "$probe/src"
cp Cargo.lock "$probe/Cargo.lock"
cat > "$probe/Cargo.toml" <<MANIFEST
[package]
name = "cyrus-core-boundary-check"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
cyrus-assets = { path = "$root/crates/cyrus-assets" }
cyrus-world = { path = "$root/crates/cyrus-world" }
cyrus-game = { path = "$root/crates/cyrus-game" }
MANIFEST
cat > "$probe/src/lib.rs" <<'RUST'
pub use cyrus_assets::animation::Animations;
pub use cyrus_world::{animation::AnimationPlugin, lifecycle::WorldPlugin};
pub use cyrus_game::GamePlugin;
RUST
cargo tree --manifest-path "$probe/Cargo.toml" --edges normal,build --prefix none > "$probe/tree"
if grep -E '^(bevy_(render|window|winit|audio|pbr|gltf|mesh|image)|redguard-preservation|cyrus-(render|control)|wgpu|winit|cpal|rodio) ' "$probe/tree"; then
    echo 'core boundary: a rendering, install-parser or transport dependency reached the core' >&2
    exit 1
fi
CARGO_TARGET_DIR="$root/target/core-boundary-build" cargo check --manifest-path "$probe/Cargo.toml"
echo 'core boundary: portable consumer passed'
