#!/usr/bin/env bash
# Builds requested applications together into one web root with shared asset URLs.
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/scripts/toolchain.sh
. dev/scripts/cargo.sh
# CARGO_TARGET_DIR may point outside the checkout; the web root stays in target/web.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
apps=("${@:-cyrus-web}")
profile="${CYRUS_WASM_PROFILE:-wasm-dev}"
packages=(-p cyrus-assets-worker)
for app in "${apps[@]}"; do
    case "$app" in cyrus-web|cyrus-viewer) ;; *) echo "unknown web app: $app" >&2; exit 2 ;; esac
    packages+=(-p "$app")
done
if [ -n "${CYRUS_WEB_CHECKPOINT_DIR:-}" ]; then
    mkdir -p "$CYRUS_WEB_CHECKPOINT_DIR"
    exec {checkpoint_lock}>"$CYRUS_WEB_CHECKPOINT_DIR/build.lock"
    flock -n "$checkpoint_lock" || { echo "checkpoint build already running" >&2; exit 1; }
fi
provenance=$(mktemp)
trap 'rm -f "$provenance"' EXIT
python3 dev/scripts/build-provenance.py snapshot "$provenance"
rm -f target/web/build-provenance.json
# Profiling checkpoints live on a caller-selected persistent host mount. Normal
# builds retain the single Cargo invocation and do not use the checkpoint path.
keep_debug=()
if [ "${CYRUS_WEB_KEEP_DEBUG:-0}" = 1 ]; then
    keep_debug=(--keep-debug)
fi
if [ -n "${CYRUS_WEB_CHECKPOINT_DIR:-}" ]; then
    configuration="profile=$profile
$(rustc -vV)
$(env | LC_ALL=C sort | rg '^(CARGO_PROFILE_|CARGO_BUILD_RUSTFLAGS=|CARGO_ENCODED_RUSTFLAGS=|RUSTFLAGS=|RUSTC=|RUSTC_WRAPPER=|CYRUS_WASM_PROFILE=|CYRUS_WEB_KEEP_DEBUG=)' || true)"
    for package in cyrus-assets-worker "${apps[@]}"; do
        artifact="$CARGO_TARGET_DIR/wasm32-unknown-unknown/$profile/$package.wasm"
        if [ "$package" = cyrus-assets-worker ]; then
            artifact="$CARGO_TARGET_DIR/wasm32-unknown-unknown/$profile/cyrus_assets_worker.wasm"
        fi
        if python3 dev/scripts/web-build-checkpoint.py restore "$CYRUS_WEB_CHECKPOINT_DIR" \
            "$provenance" "$configuration" "$artifact"; then
            echo "restored completed $package WASM checkpoint"
        else
            status=$?
            if [ "$status" != 3 ]; then exit "$status"; fi
            cargo build -p "$package" --target wasm32-unknown-unknown --profile "$profile"
            # Reject source races before preserving a reusable compiler result.
            current=$(mktemp)
            python3 dev/scripts/build-provenance.py snapshot "$current"
            if ! cmp -s "$provenance" "$current"; then
                rm -f "$current"
                echo "source changed during build; checkpoint rejected" >&2
                exit 1
            fi
            rm -f "$current"
            python3 dev/scripts/web-build-checkpoint.py store "$CYRUS_WEB_CHECKPOINT_DIR" \
                "$provenance" "$configuration" "$artifact"
        fi
    done
else
    cargo build "${packages[@]}" --target wasm32-unknown-unknown --profile "$profile"
fi
out=target/web
mkdir -p "$out"
artifacts=()
for app in "${apps[@]}"; do
    case "$app" in cyrus-web) page=index.html ;; cyrus-viewer) page=viewer.html ;; esac
    wasm-bindgen "${keep_debug[@]}" --target web --no-typescript --out-dir "$out/pkg" --out-name "$app" \
        "$CARGO_TARGET_DIR/wasm32-unknown-unknown/$profile/$app.wasm"
    cp "crates/$app/web/index.html" "$out/$page"
    artifacts+=("$out/$page" "$out/pkg/$app.js" "$out/pkg/${app}_bg.wasm")
    echo "built $out ($(du -h "$out/pkg/${app}_bg.wasm" | cut -f1) wasm)"
done
wasm-bindgen "${keep_debug[@]}" --target web --no-typescript --out-dir "$out/pkg" --out-name cyrus-assets-worker \
    "$CARGO_TARGET_DIR/wasm32-unknown-unknown/$profile/cyrus_assets_worker.wasm"
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
if [ -L "$out/game-config" ]; then
    rm "$out/game-config"
elif [ -L "$out/game-config/cache" ]; then
    rm "$out/game-config/cache"
fi
