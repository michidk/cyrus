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
        rm -f "target/artifacts/cyrus-viewer-$platform".{tar.gz,exe}
        apps=(cyrus)
        if [[ -f crates/cyrus-viewer/Cargo.toml ]]; then apps+=(cyrus-viewer); fi
        packages=()
        for app in "${apps[@]}"; do packages+=(-p "$app"); done
        cargo build --locked --release --target "$platform" "${packages[@]}"
        suffix=""
        if [[ "$platform" = *-windows-* ]]; then suffix=.exe; fi
        # Windows gets the bare executable; Unix archives keep the executable bit.
        for app in "${apps[@]}"; do
            binary="target/$platform/release/$app$suffix"
            if [[ -n "$suffix" ]]; then
                cp "$binary" "target/artifacts/$app-$platform$suffix"
            else
                tar -czf "target/artifacts/$app-$platform.tar.gz" -C "$(dirname "$binary")" "$app"
            fi
        done
        ;;
    web)
        apps=(cyrus-web)
        if [[ -f crates/cyrus-viewer/Cargo.toml ]]; then apps+=(cyrus-viewer); fi
        CYRUS_WASM_PROFILE=wasm-release bash dev/scripts/build-web.sh "${apps[@]}"
        mkdir -p "$stage/browser/pkg"
        cp target/web/index.html target/web/sw.js target/web/loader.js target/web/install.js \
            target/web/loading.css target/web/asset-worker.js target/web/game-assets.js dev/scripts/viewer-server.py "$stage/browser/"
        cp crates/cyrus-web/README.md "$stage/browser/README.md"
        for app in "${apps[@]}"; do
            cp "target/web/pkg/$app.js" "target/web/pkg/${app}_bg.wasm" "$stage/browser/pkg/"
        done
        if [[ -d target/web/pkg/snippets ]]; then
            cp -R target/web/pkg/snippets "$stage/browser/pkg/"
        fi
        cp target/web/pkg/cyrus-assets-worker.js target/web/pkg/cyrus-assets-worker_bg.wasm "$stage/browser/pkg/"
        if [[ -f crates/cyrus-viewer/Cargo.toml ]]; then
            cp target/web/viewer.html "$stage/browser/"
            cp crates/cyrus-viewer/README.md "$stage/browser/VIEWER.md"
        fi
        tar -czf target/artifacts/cyrus-browser.tar.gz -C "$stage" browser
        ;;
    *) echo 'usage: build-artifacts.sh native <Rust target> | web' >&2; exit 2 ;;
esac
