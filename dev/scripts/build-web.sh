#!/usr/bin/env bash
# Builds crates/cyrus-web for wasm32 into target/web/ (page, pkg/, and a link to the GLB cache).
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build -p cyrus-web --target wasm32-unknown-unknown --profile wasm-release
out=target/web
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" --out-name cyrus-web \
    target/wasm32-unknown-unknown/wasm-release/cyrus-web.wasm
cp crates/cyrus-web/web/index.html crates/cyrus-web/web/sw.js "$out/"
ln -sfn "$PWD/cyrus-config" "$out/cyrus-config"
echo "built $out ($(du -h "$out/pkg/cyrus-web_bg.wasm" | cut -f1) wasm)"
