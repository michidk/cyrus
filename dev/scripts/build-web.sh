#!/usr/bin/env bash
# Builds requested applications together into one web root with shared asset URLs.
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/scripts/toolchain.sh
. dev/scripts/cargo.sh
export CARGO_TARGET_DIR="$PWD/target"
apps=("${@:-cyrus-web}")
profile="${CYRUS_WASM_PROFILE:-wasm-dev}"
packages=(-p cyrus-assets-worker)
for app in "${apps[@]}"; do
    case "$app" in cyrus-web|cyrus-viewer) ;; *) echo "unknown web app: $app" >&2; exit 2 ;; esac
    packages+=(-p "$app")
done
provenance=$(mktemp)
trap 'rm -f "$provenance"' EXIT
python3 dev/scripts/build-provenance.py snapshot "$provenance"
rm -f target/web/build-provenance.json
cargo build "${packages[@]}" --target wasm32-unknown-unknown --profile "$profile"
out=target/web
mkdir -p "$out"
artifacts=()
for app in "${apps[@]}"; do
    case "$app" in cyrus-web) page=index.html ;; cyrus-viewer) page=viewer.html ;; esac
    wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" --out-name "$app" \
        "target/wasm32-unknown-unknown/$profile/$app.wasm"
    cp "crates/$app/web/index.html" "$out/$page"
    artifacts+=("$out/$page" "$out/pkg/$app.js" "$out/pkg/${app}_bg.wasm")
    echo "built $out ($(du -h "$out/pkg/${app}_bg.wasm" | cut -f1) wasm)"
done
wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" --out-name cyrus-assets-worker \
    target/wasm32-unknown-unknown/$profile/cyrus_assets_worker.wasm
cp dev/web/asset-worker.js dev/web/game-assets.js "$out/"
cp dev/web/loader.js dev/web/install.js dev/web/local-install.js dev/web/sw.js dev/web/loading.css "$out/"
artifacts+=("$out/pkg/cyrus-assets-worker.js" "$out/pkg/cyrus-assets-worker_bg.wasm")
for file in asset-worker.js game-assets.js loader.js install.js local-install.js sw.js loading.css; do
    artifacts+=("$out/$file")
done
if [ -d "$out/pkg/snippets" ]; then
    while IFS= read -r -d '' file; do
        artifacts+=("$file")
    done < <(find "$out/pkg/snippets" -type f -print0 | sort -z)
fi
python3 dev/scripts/build-provenance.py seal "$provenance" "${artifacts[@]}"
mv "$provenance" "$out/build-provenance.json"
# Browser code has no implicit link to installed files or exported caches.
if [ -L "$out/cyrus-config" ]; then
    rm "$out/cyrus-config"
elif [ -L "$out/cyrus-config/cache" ]; then
    rm "$out/cyrus-config/cache"
fi
