#!/usr/bin/env bash
# Restore an explicitly supplied local runtime archive after an ephemeral Docker reset.
set -euo pipefail
if (( $# != 2 )); then
    echo 'usage: ensure-runtime-image.sh IMAGE LOCAL_ARCHIVE' >&2
    exit 2
fi
image="$1"
archive="$2"
if docker image inspect "$image" >/dev/null 2>&1; then
    exit 0
fi
if [[ ! -s "$archive" ]]; then
    echo "runtime image $image is unavailable and archive is missing or empty: $archive" >&2
    exit 1
fi
# Several sessions may discover the same reset. Serialize restore and recheck ownership.
exec 8>"$archive.restore.lock"
flock 8
if ! docker image inspect "$image" >/dev/null 2>&1; then
    docker load --input "$archive"
fi
if ! docker image inspect "$image" >/dev/null 2>&1; then
    echo "runtime archive $archive did not restore requested image $image" >&2
    exit 1
fi
