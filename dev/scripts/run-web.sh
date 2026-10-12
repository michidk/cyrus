#!/usr/bin/env bash
# Shared browser server with optional install location for unattended launches.
set -euo pipefail
cd "$(dirname "$0")/../.."
app=cyrus-web
case "${1:-}" in cyrus-web|cyrus-viewer) app="$1"; shift ;; esac
if [ ! -f "target/web/pkg/${app}_bg.wasm" ] || [ ! -f target/web/install.js ]; then
    dev/scripts/build-web.sh "$app"
fi
if [ "$app" = cyrus-viewer ]; then
    # Keep the existing positional install invocation working.
    if [ "$#" -gt 0 ] && [[ "$1" != --* ]]; then set -- --install "$@"; fi
    exec vibepod-preview run -port 36086 -title "cyrus asset viewer" cyrus-viewer -- \
        python3 dev/scripts/viewer-server.py --page viewer.html --port 36086 "$@"
fi
exec vibepod-preview run -port 36082 -title "cyrus (web)" cyrus-web -- \
    python3 dev/scripts/viewer-server.py --page index.html --port 36082 "$@"
