#!/usr/bin/env bash
# cyrus with a virtual display and software Vulkan, as a background container (no preview;
# the browser build behind "cyrus-web" is the visual one). This exists for the control port
# (127.0.0.1:7117) that the MCP server, smoke tests and compare.sh use for real rendering.
# Builds locally, runs the binary inside dev/docker/dev (the container has the runtime libs).
set -euo pipefail
cd "$(dirname "$0")/../.."
viewport=()
if [ "${1:-}" = --fixture ]; then
    fixture=${2:?--fixture requires a matched JSON fixture}
    mapfile -t launch < <(node dev/scripts/matched-capture.mjs "$fixture" unused --launch-args)
    [ "${#launch[@]}" -gt 0 ] || exit 1
    mapfile -t viewport < <(node -e 'console.log(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")).viewport.join("\n"))' "$fixture")
    shift 2
    set -- "${launch[@]}" "$@"
fi
bash dev/scripts/build-native.sh
# Always build: the layer cache makes it quick, and a changed Dockerfile is never missed.
docker build -q -t cyrus-dev:dev dev/docker/dev >/dev/null
docker rm -f cyrus-dev >/dev/null 2>&1 || true
install=$(readlink -f "${REDGUARD_DIR:-redguard}")
startup_mount=()
if [ -d "$install/../DOSBOX" ]; then
    startup_mount=(-v "$install/../DOSBOX:/DOSBOX:ro")
fi
movie_mount=()
if [ -f "$install/../game.gog" ]; then
    movie_mount=(-v "$install/../game.gog:/game.gog:ro")
fi
docker run -d --rm --name cyrus-dev -p 127.0.0.1:7117:7117 \
    "${startup_mount[@]}" "${movie_mount[@]}" -v "$PWD:/repo" -v "$install:/game:ro" -w /repo -e CYRUS_CONFIG_DIR=/repo/game-config -e BEVY_ASSET_ROOT=/repo -e REDGUARD_DIR=/game -e CYRUS_CONTROL_ADDR=0.0.0.0:7117 \
    cyrus-dev:dev /repo/target/debug/cyrus "$@" >/dev/null
echo "cyrus-dev running; control port 127.0.0.1:7117; logs: docker logs -f cyrus-dev; stop: docker rm -f cyrus-dev"
if [ "${#viewport[@]}" -eq 2 ]; then
    for _ in {1..60}; do
        if docker exec -e DISPLAY=:1 cyrus-dev xdotool search --name '^cyrus$' windowsize "${viewport[@]}" 2>/dev/null; then
            exit 0
        fi
        sleep 1
    done
    echo 'capture window did not appear' >&2
    exit 1
fi
