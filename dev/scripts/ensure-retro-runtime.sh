#!/usr/bin/env bash
# Recover the standard original-game rig from a host-retained, source-keyed image.
set -euo pipefail
repo=$(cd "$(dirname "$0")/../.." && pwd)
# Explicit retained runtimes use the same override as run-retro.sh. Keep their
# archive in place; --retain writes Docker save format and must not replace a
# supplied compressed archive or manufacture a source-keyed alias.
if [[ -n "${CYRUS_RETRO_IMAGE:-}" || -n "${CYRUS_RETRO_IMAGE_ARCHIVE:-}" ]]; then
    if [[ -z "${CYRUS_RETRO_IMAGE:-}" || -z "${CYRUS_RETRO_IMAGE_ARCHIVE:-}" ]]; then
        echo 'explicit retro recovery requires both CYRUS_RETRO_IMAGE and CYRUS_RETRO_IMAGE_ARCHIVE' >&2
        exit 2
    fi
    "$repo/dev/scripts/ensure-runtime-image.sh" "$CYRUS_RETRO_IMAGE" "$CYRUS_RETRO_IMAGE_ARCHIVE" >&2
    printf '%s\n' "$CYRUS_RETRO_IMAGE"
    exit 0
fi
fingerprint=$(cat "$repo/dev/docker/retro/Dockerfile" "$repo/dev/docker/retro/entrypoint.sh" \
    "$repo/dev/docker/retro/dosbox-x.conf" | sha256sum | cut -d' ' -f1)
image="cyrus-retro:$fingerprint"
cache="${CYRUS_MAGIC:-$repo/magic}/build-tools"
archive="$cache/cyrus-retro-$fingerprint.tar"
mkdir -p "$cache"
exec 7>"$archive.build.lock"
flock 7
if ! docker image inspect "$image" >/dev/null 2>&1; then
    if [[ -s "$archive" ]]; then
        "$repo/dev/scripts/ensure-runtime-image.sh" "$image" "$archive" >&2
    else
        docker build -t "$image" "$repo/dev/docker/retro" >&2
    fi
fi
"$repo/dev/scripts/ensure-runtime-image.sh" --retain "$image" "$archive" >&2
printf '%s\n' "$image"
