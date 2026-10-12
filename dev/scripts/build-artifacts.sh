#!/usr/bin/env bash
# Package application code only; installed files and local caches stay outside archives.
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/scripts/cargo.sh
mkdir -p target/artifacts
stage=$(mktemp -d target/artifacts/package.XXXXXX)
trap 'rm -rf "$stage"' EXIT

case "${1:-}" in
    native)
        platform="${2:?Rust target required}"
        rm -f "target/artifacts/cyrus-viewer-$platform.tar.gz"
        apps=(cyrus)
        if [[ -f crates/cyrus-viewer/Cargo.toml ]]; then apps+=(cyrus-viewer); fi
        packages=()
        for app in "${apps[@]}"; do packages+=(-p "$app"); done
        cargo build --locked --release --target "$platform" "${packages[@]}"
        suffix=""
        if [[ "$platform" = *-windows-* ]]; then suffix=.exe; fi
        for app in "${apps[@]}"; do
            mkdir -p "$stage/$app"
            cp "target/$platform/release/$app$suffix" "$stage/$app/"
            cp "crates/$app/README.md" "$stage/$app/README.md"
            tar -czf "target/artifacts/$app-$platform.tar.gz" -C "$stage" "$app"
        done
        ;;
    web)
        apps=(cyrus-web)
        if [[ -f crates/cyrus-viewer/Cargo.toml ]]; then apps+=(cyrus-viewer); fi
        bash dev/scripts/build-web.sh "${apps[@]}"
        mkdir -p "$stage/browser/pkg"
        cp target/web/index.html target/web/sw.js target/web/audio.js "$stage/browser/"
        cp crates/cyrus-web/README.md "$stage/browser/README.md"
        for app in "${apps[@]}"; do
            cp "target/web/pkg/$app.js" "target/web/pkg/${app}_bg.wasm" "$stage/browser/pkg/"
        done
        if [[ -d target/web/pkg/snippets ]]; then
            cp -R target/web/pkg/snippets "$stage/browser/pkg/"
        fi
        if [[ -f crates/cyrus-viewer/Cargo.toml ]]; then
            cp target/web/viewer.html target/web/loader.js target/web/loading.css \
                target/web/asset-worker.js "$stage/browser/"
            cp target/web/pkg/cyrus-assets-worker.js target/web/pkg/cyrus-assets-worker_bg.wasm \
                "$stage/browser/pkg/"
            cp crates/cyrus-viewer/README.md "$stage/browser/VIEWER.md"
            cp dev/scripts/viewer-server.py "$stage/browser/"
        fi
        tar -czf target/artifacts/cyrus-browser.tar.gz -C "$stage" browser
        ;;
    *) echo 'usage: build-artifacts.sh native <Rust target> | web' >&2; exit 2 ;;
esac
