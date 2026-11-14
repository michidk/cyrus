#!/usr/bin/env bash
# Restore an explicitly supplied local runtime archive after an ephemeral Docker reset.
set -euo pipefail
retain=0
if [[ "${1:-}" == --retain ]]; then
    retain=1
    shift
fi
if (( $# != 2 )); then
    echo 'usage: ensure-runtime-image.sh [--retain] IMAGE LOCAL_ARCHIVE' >&2
    exit 2
fi
image="$1"
archive="$2"
if (( ! retain )) && docker image inspect "$image" >/dev/null 2>&1; then
    exit 0
fi
if (( ! retain )) && [[ ! -s "$archive" ]]; then
    echo "runtime image $image is unavailable and archive is missing or empty: $archive" >&2
    exit 1
fi
# Several sessions may discover the same reset. Serialize restore and recheck ownership.
if (( retain )); then mkdir -p "$(dirname "$archive")"; fi
exec 8>"$archive.restore.lock"
flock 8
if ! docker image inspect "$image" >/dev/null 2>&1; then
    if [[ ! -s "$archive" ]]; then
        echo "runtime image $image is unavailable and archive is missing or empty: $archive" >&2
        exit 1
    fi
    docker load --input "$archive"
fi
if ! docker image inspect "$image" >/dev/null 2>&1; then
    echo "runtime archive $archive did not restore requested image $image" >&2
    exit 1
fi

# Opt-in retention also checkpoints an image already available after setup.
# Publish only a completed Docker save; an interrupted attempt is never reusable.
if (( retain )) && [[ ! -s "$archive" ]]; then
    partial=$(mktemp "$archive.partial.XXXXXX")
    trap 'rm -f "$partial"' EXIT
    docker save --output "$partial" "$image"
    [[ -s "$partial" ]] || { echo "Docker saved an empty runtime archive: $image" >&2; exit 1; }
    mv "$partial" "$archive"
fi
