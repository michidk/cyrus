#!/usr/bin/env bash
# Serves target/web behind the VibePod preview "cyrus-web". Export startup or scenes first.
set -euo pipefail
cd "$(dirname "$0")/../.."
[ -f target/web/pkg/cyrus-web_bg.wasm ] || dev/scripts/build-web.sh
[ -f cyrus-config/cache/startup/title.png ] || ls cyrus-config/cache/*.glb >/dev/null 2>&1 || {
    echo "no exported assets; run cargo run -p cyrus -- --export-startup" >&2
    exit 1
}
vibepod-preview run -port 36082 -title "cyrus (web)" cyrus-web -- python3 -m http.server 36082 --bind 127.0.0.1 -d target/web
