#!/usr/bin/env bash
# cyrus with a virtual display and software Vulkan, as a background container (no preview;
# the browser build behind "cyrus-web" is the visual one). This exists for the control port
# (127.0.0.1:7117) that the MCP server, smoke tests and compare.sh use for real rendering.
# Builds locally, runs the binary inside dev/docker/dev (the container has the runtime libs).
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/scripts/cargo.sh
cargo build -p cyrus
# Always build: the layer cache makes it quick, and a changed Dockerfile is never missed.
docker build -q -t cyrus-dev:dev dev/docker/dev >/dev/null
docker rm -f cyrus-dev >/dev/null 2>&1 || true
install=$(readlink -f "${REDGUARD_DIR:-redguard}")
docker run -d --rm --name cyrus-dev -p 127.0.0.1:7117:7117 \
    -v "$PWD:/repo" -v "$install:/game:ro" -w /repo -e BEVY_ASSET_ROOT=/repo -e REDGUARD_DIR=/game -e CYRUS_CONTROL_ADDR=0.0.0.0:7117 \
    cyrus-dev:dev /repo/target/debug/cyrus "$@" >/dev/null
echo "cyrus-dev running; control port 127.0.0.1:7117; logs: docker logs -f cyrus-dev; stop: docker rm -f cyrus-dev"
