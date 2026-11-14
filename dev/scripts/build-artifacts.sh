#!/usr/bin/env bash
# Package application code only; installed files and local caches stay outside archives.
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/scripts/cargo.sh
build_root="${CARGO_TARGET_DIR:-target}"
artifacts="$build_root/artifacts"
mkdir -p "$artifacts"
stage=$(mktemp -d "$artifacts/package.XXXXXX")
trap 'rm -rf "$stage"' EXIT

case "${1:-}" in
    native|native-mods)
        platform="${2:?Rust target required}"
        rm -f "$artifacts/cyrus-viewer-$platform".{tar.gz,exe}
        if [[ "$1" = native-mods ]]; then
            rm -f "$artifacts/cyrus-with-mods-$platform".{tar.gz,zip}
        fi
        apps=(cyrus)
        if [[ -f crates/cyrus-viewer/Cargo.toml ]]; then apps+=(cyrus-viewer); fi
        packages=()
        for app in "${apps[@]}"; do packages+=(-p "$app"); done
        cargo build --locked --release --target "$platform" "${packages[@]}"
        suffix=""
        if [[ "$platform" = *-windows-* ]]; then suffix=.exe; fi
        # Windows gets the bare executable; Unix archives keep the executable bit.
        for app in "${apps[@]}"; do
            binary="$build_root/$platform/release/$app$suffix"
            if [[ -n "$suffix" ]]; then
                cp "$binary" "$artifacts/$app-$platform$suffix"
            else
                tar -czf "$artifacts/$app-$platform.tar.gz" -C "$(dirname "$binary")" "$app"
            fi
        done
        if [[ "$1" = native-mods ]]; then
            bundle="$stage/bundled"
            mkdir -p "$bundle/game-config/mods"
            cp "$build_root/$platform/release/cyrus$suffix" "$bundle/"
            count=0
            for manifest in mods/*/mod.toml; do
                [[ -f "$manifest" ]] || continue
                dir="${manifest%/mod.toml}"
                id="${dir##*/}"
                out="$bundle/game-config/mods/$id"
                mkdir -p "$out"
                cp "$manifest" "$out/"
                if [[ -f "$dir/Cargo.toml" ]]; then
                    metadata=$(cargo metadata --locked --no-deps --format-version 1 --manifest-path "$dir/Cargo.toml")
                    guest_target=$(printf '%s' "$metadata" | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
                    library=$(printf '%s' "$metadata" | python3 -c 'import json,sys; print(next(t["name"] for p in json.load(sys.stdin)["packages"] for t in p["targets"] if "cdylib" in t["kind"]))')
                    cargo build --locked --release --target wasm32-wasip2 --manifest-path "$dir/Cargo.toml"
                    cp "$guest_target/wasm32-wasip2/release/$library.wasm" "$out/mod.wasm"
                fi
                for shader in "$dir"/*.wesl; do
                    [[ ! -f "$shader" ]] || cp "$shader" "$out/"
                done
                if [[ -d "$dir/files" ]]; then cp -a "$dir/files" "$out/"; fi
                count=$((count + 1))
            done
            if (( count == 0 )); then echo 'No bundled mod manifests found' >&2; exit 1; fi
            printf '{"active":[]}\n' > "$bundle/game-config/mods.json"
            cat > "$bundle/MODS.txt" <<'EOF'
This package contains cyrus and every bundled mod in this source revision.
Extract the entire archive and run cyrus from the extracted directory so it uses
the included game-config folder. CYRUS_CONFIG_DIR overrides that location.
Open F11, then Mods, to enable mods and adjust their settings.
Mods are installed but disabled by default. Choose which to enable; movement
and camera mods can conflict when enabled together.
The original game files are not included. Supply your own GOG installation.
EOF
            if [[ -n "$suffix" ]]; then
                python3 - "$bundle" "$artifacts/cyrus-with-mods-$platform.zip" <<'PY'
from pathlib import Path
import sys, zipfile
root, destination = map(Path, sys.argv[1:])
with zipfile.ZipFile(destination, 'w', zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(root.rglob('*')):
        if path.is_file():
            archive.write(path, path.relative_to(root).as_posix())
PY
            else
                tar -czf "$artifacts/cyrus-with-mods-$platform.tar.gz" -C "$bundle" .
            fi
        fi
        ;;
    web)
        apps=(cyrus-web)
        if [[ -f crates/cyrus-viewer/Cargo.toml ]]; then apps+=(cyrus-viewer); fi
        CYRUS_WASM_PROFILE=wasm-release bash dev/scripts/build-web.sh "${apps[@]}"
        mkdir -p "$stage/browser/pkg"
        cp target/web/index.html target/web/sw.js target/web/loader.js target/web/install.js target/web/local-install.js \
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
        tar -czf "$artifacts/cyrus-browser.tar.gz" -C "$stage" browser
        ;;
    *) echo 'usage: build-artifacts.sh native|native-mods <Rust target> | web' >&2; exit 2 ;;
esac
