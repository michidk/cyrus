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
    native|native-mods|native-raytracing)
        platform="${2:?Rust target required}"
        artifact_suffix=""
        build_args=(--locked --release --target "$platform")
        if [[ "$1" = native-raytracing ]]; then
            case "$platform" in
                x86_64-unknown-linux-gnu|x86_64-pc-windows-msvc) ;;
                *) echo "Ray-tracing packages are supported on Linux x64 and Windows x64: $platform" >&2; exit 2 ;;
            esac
            artifact_suffix=-raytracing
            build_args+=(--features cyrus/raytracing)
            rm -f "$artifacts/cyrus-raytracing-$platform".{tar.gz,zip}
        else
            rm -f "$artifacts/cyrus-viewer-$platform".{tar.gz,exe,zip}
            rm -f "$artifacts/cyrus-$platform.exe"
        fi
        if [[ "$1" = native-mods ]]; then
            rm -f "$artifacts/cyrus-with-mods-$platform".{tar.gz,zip}
        fi
        apps=(cyrus)
        if [[ "$1" != native-raytracing && -f crates/cyrus-viewer/Cargo.toml ]]; then apps+=(cyrus-viewer); fi
        launcher=false
        if [[ "$1" = native-mods && -f crates/cyrus-launcher/Cargo.toml ]]; then
            apps+=(cyrus-launcher)
            launcher=true
        fi
        packages=()
        for app in "${apps[@]}"; do packages+=(-p "$app"); done
        cargo build "${build_args[@]}" "${packages[@]}"
        suffix=""
        if [[ "$platform" = *-windows-* ]]; then suffix=.exe; fi
        # Compress every download; Unix archives also keep the executable bit.
        for app in "${apps[@]}"; do
            # The launcher needs a sibling game; ship it in the complete bundle.
            [[ "$app" != cyrus-launcher ]] || continue
            binary="$build_root/$platform/release/$app$suffix"
            if [[ -n "$suffix" ]]; then
                python3 - "$binary" "$artifacts/$app$artifact_suffix-$platform.zip" <<'PYZIP'
from pathlib import Path
import sys, zipfile
binary, destination = map(Path, sys.argv[1:])
with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED) as archive:
    archive.write(binary, binary.name)
PYZIP
            else
                tar -czf "$artifacts/$app$artifact_suffix-$platform.tar.gz" -C "$(dirname "$binary")" "$app"
            fi
        done
        if [[ "$1" = native-mods ]]; then
            bundle="$stage/bundled"
            mkdir -p "$bundle/mods"
            cp "$build_root/$platform/release/cyrus$suffix" "$bundle/"
            if [[ "$launcher" = true ]]; then
                cp "$build_root/$platform/release/cyrus-launcher$suffix" "$bundle/"
            fi
            count=0
            for manifest in mods/*/mod.toml; do
                [[ -f "$manifest" ]] || continue
                dir="${manifest%/mod.toml}"
                id="${dir##*/}"
                out="$bundle/mods/$id"
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
            cat > "$bundle/README.txt" <<'EOF'
Cyrus: a native rebuild of The Elder Scrolls Adventures: Redguard

You need your own copy of the game (for example from GOG). It is not included.
Extract the whole archive into a folder of its own that you can write to, not
inside your game folder, and keep all files together.

Start cyrus-launcher. It looks for your game by itself: the folder it remembered
from an earlier start, the REDGUARD_DIR setting, its own folder and the folders
around it, and the usual GOG folders. If it finds none, it asks once for the
folder that contains your game and remembers your choice. It then shows the
launcher, where you choose game options and mods; Play starts the game.

You can also start cyrus directly. Options such as --fullscreen are listed by
cyrus --help.

The mods folder beside the programs holds the bundled mods; put your own mods
there too, one folder each. Settings are stored per user, in your system's usual place; CYRUS_CONFIG_DIR overrides
the location. Every bundled mod starts disabled: enable the ones you want in the
launcher or in the game (Backtick, then Mods). Movement and camera mods can
conflict when enabled together.

Reports are welcome at https://github.com/michidk/cyrus/issues
EOF
            # Never retain links to builder-local files or let zip dereference them.
            python3 - "$bundle" <<'PYLINKS'
from pathlib import Path
import sys
root = Path(sys.argv[1])
for path in root.rglob('*'):
    if path.is_symlink():
        sys.exit(f"Cannot package symbolic link: {path.relative_to(root)}")
PYLINKS
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
    *) echo 'usage: build-artifacts.sh native|native-mods|native-raytracing <Rust target> | web' >&2; exit 2 ;;
esac
