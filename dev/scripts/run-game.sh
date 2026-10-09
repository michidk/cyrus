#!/usr/bin/env bash
# cyrus with a virtual display and software Vulkan, behind the VibePod preview "cyrus".
# Builds locally, runs the binary inside dev/docker/dev (the container has the runtime libs).
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build -p cyrus
docker image inspect cyrus-dev:dev >/dev/null 2>&1 || docker build -t cyrus-dev:dev dev/docker/dev
docker rm -f cyrus-dev >/dev/null 2>&1 || true
install=$(readlink -f "${REDGUARD_DIR:-redguard}")
vibepod-preview run -port 36081 -title "cyrus (Bevy)" cyrus -- \
    docker run --rm --name cyrus-dev -p 127.0.0.1:36081:6080 -p 127.0.0.1:7117:7117 \
        -v "$PWD:/repo" -v "$install:/game:ro" -w /repo -e REDGUARD_DIR=/game -e CYRUS_CONTROL_ADDR=0.0.0.0:7117 \
        cyrus-dev:dev /repo/target/debug/cyrus "$@"
