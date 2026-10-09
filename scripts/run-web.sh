#!/usr/bin/env bash
# Serves target/web behind the VibePod preview "cyrus-web". Export scenes first (native game or `just run-headless`).
set -euo pipefail
cd "$(dirname "$0")/.."
[ -f target/web/pkg/cyrus-web_bg.wasm ] || scripts/build-web.sh
ls cyrus-config/cache/*.glb >/dev/null 2>&1 || { echo "no exported scenes in cyrus-config/cache; run the native game once" >&2; exit 1; }
vibepod-preview run -port 36082 -title "cyrus (web)" cyrus-web -- python3 -m http.server 36082 --bind 127.0.0.1 -d target/web
