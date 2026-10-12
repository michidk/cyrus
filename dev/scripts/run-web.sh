#!/usr/bin/env bash
# Serves target/web behind the VibePod preview "cyrus-web". Export startup or scenes first.
set -euo pipefail
cd "$(dirname "$0")/../.."
app="${1:-cyrus-web}"
[ -f "target/web/pkg/${app}_bg.wasm" ] || dev/scripts/build-web.sh "$app"
if [ "$app" = cyrus-viewer ]; then
    install="${2:-${REDGUARD_DIR:-}}"
    [ -n "$install" ] || { echo "supply the original install path or REDGUARD_DIR" >&2; exit 1; }
    exec vibepod-preview run -port 36086 -title "cyrus asset viewer" cyrus-viewer -- \
        python3 dev/scripts/viewer-server.py --install "$install" --port 36086
fi
vibepod-preview run -port 36082 -title "cyrus (web)" cyrus-web -- python3 dev/scripts/web-server.py --port 36082 --bind 127.0.0.1 --directory target/web
