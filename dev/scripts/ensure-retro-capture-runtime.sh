#!/usr/bin/env bash
# Recover the SDL presentation runtime without holding a live-game lock.
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
fingerprint=$(cat "$repo/dev/docker/retro/Dockerfile.capture" \
    "$repo/dev/docker/retro/Dockerfile.capture.dockerignore" \
    "$repo/dev/docker/retro/contact-entrypoint.sh" "$repo/dev/docker/retro/dosbox-x.conf" \
    "$repo/dev/scripts/sdl-frame-capture.c" "$repo/dev/scripts/sdl-mixer-capture.c" \
    | sha256sum | cut -d' ' -f1)
image="cyrus-retro-capture:$fingerprint"
archive="$repo/magic/build-tools/cyrus-retro-capture-$fingerprint.tar"
mkdir -p "$(dirname "$archive")"
exec 7>"$archive.build.lock"
flock 7
if ! docker image inspect "$image" >/dev/null 2>&1; then
    if [[ -s "$archive" ]]; then
        "$repo/dev/scripts/ensure-runtime-image.sh" "$image" "$archive" >&2
    else
        docker build -t "$image" -f "$repo/dev/docker/retro/Dockerfile.capture" "$repo" >&2
    fi
fi
"$repo/dev/scripts/ensure-runtime-image.sh" --retain "$image" "$archive" >&2
printf '%s\n' "$image"
