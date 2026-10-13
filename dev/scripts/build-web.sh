#!/usr/bin/env bash
# Builds requested applications together into one web root with shared asset URLs.
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/scripts/toolchain.sh
. dev/scripts/cargo.sh
apps=("${@:-cyrus-web}")
profile="${CYRUS_WASM_PROFILE:-wasm-dev}"
packages=(-p cyrus-assets-worker)
for app in "${apps[@]}"; do
    case "$app" in cyrus-web|cyrus-viewer) ;; *) echo "unknown web app: $app" >&2; exit 2 ;; esac
    packages+=(-p "$app")
done
cargo build "${packages[@]}" --target wasm32-unknown-unknown --profile "$profile"
out=target/web
mkdir -p "$out"
for app in "${apps[@]}"; do
    case "$app" in cyrus-web) page=index.html ;; cyrus-viewer) page=viewer.html ;; esac
    wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" --out-name "$app" \
        "target/wasm32-unknown-unknown/$profile/$app.wasm"
    cp "crates/$app/web/index.html" "$out/$page"
    echo "built $out ($(du -h "$out/pkg/${app}_bg.wasm" | cut -f1) wasm)"
done
wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" --out-name cyrus-assets-worker \
    target/wasm32-unknown-unknown/$profile/cyrus_assets_worker.wasm
cp dev/web/asset-worker.js dev/web/game-assets.js "$out/"
cp dev/web/loader.js dev/web/install.js dev/web/local-install.js dev/web/sw.js dev/web/loading.css "$out/"
# Browser code has no implicit link to installed files or exported caches.
if [ -L "$out/cyrus-config" ]; then
    rm "$out/cyrus-config"
elif [ -L "$out/cyrus-config/cache" ]; then
    rm "$out/cyrus-config/cache"
fi
