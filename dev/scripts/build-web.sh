#!/usr/bin/env bash
# Builds either application into one web root with shared asset URLs.
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/scripts/toolchain.sh
. dev/scripts/cargo.sh
app="${1:-cyrus-web}"
case "$app" in cyrus-web) page=index.html ;; cyrus-viewer) page=viewer.html ;; *) echo "unknown web app: $app" >&2; exit 2 ;; esac
packages=(-p "$app")
if [[ "$app" = cyrus-viewer ]]; then packages+=(-p cyrus-assets-worker); fi
cargo build "${packages[@]}" --target wasm32-unknown-unknown --profile wasm-release
out=target/web
mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" --out-name "$app" \
    "target/wasm32-unknown-unknown/wasm-release/$app.wasm"
if [[ "$app" = cyrus-viewer ]]; then
    wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" --out-name cyrus-assets-worker \
        target/wasm32-unknown-unknown/wasm-release/cyrus_assets_worker.wasm
    cp dev/web/asset-worker.js "$out/"
fi
cp "crates/$app/web/index.html" "$out/$page"
cp dev/web/loader.js dev/web/sw.js dev/web/loading.css "$out/"
worlds=""
if [[ -d cyrus-config/cache ]]; then
    worlds="$(find cyrus-config/cache -maxdepth 1 -type f -name '*.glb' -printf '%f\n' \
        | sed -n 's/^\([A-Za-z0-9_]*\)\.glb$/\1/p' | sort -V | paste -sd, -)"
fi
sed -i "s/<meta name=\"cyrus-worlds\" content=\"\">/<meta name=\"cyrus-worlds\" content=\"$worlds\">/" "$out/$page"
if [ -L "$out/cyrus-config" ]; then rm "$out/cyrus-config"; fi
mkdir -p "$out/cyrus-config"
ln -sfn "$PWD/cyrus-config/cache" "$out/cyrus-config/cache"
echo "built $out ($(du -h "$out/pkg/${app}_bg.wasm" | cut -f1) wasm)"
