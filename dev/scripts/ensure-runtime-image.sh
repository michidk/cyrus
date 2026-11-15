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

# A mutable tag may have been rebuilt since the previous checkpoint. Compare
# the saved config digest and tag before reusing it, without extracting files.
archive_matches_image() {
    local identity
    identity=$(docker image inspect --format '{{.Id}}' "$image")
    python3 - "$archive" "$image" "$identity" <<'PYTHON'
import hashlib
import json
import sys
import tarfile

path, tag, identity = sys.argv[1:]
if ':' not in tag.rsplit('/', 1)[-1]:
    tag += ':latest'
try:
    with tarfile.open(path) as archive:
        manifest = json.load(archive.extractfile('manifest.json'))
        def config_owned_by(digest, config_digest, visited=None):
            visited = set() if visited is None else visited
            if digest in visited:
                return False
            visited.add(digest)
            if digest == config_digest:
                return True
            # Docker's containerd store identifies images by an OCI manifest
            # (or index), while its classic store uses the config digest.
            algorithm, value = digest.split(':', 1)
            if algorithm != 'sha256':
                return False
            blob = archive.extractfile(f'blobs/sha256/{value}').read()
            if hashlib.sha256(blob).hexdigest() != value:
                return False
            document = json.loads(blob)
            if document.get('config', {}).get('digest') == config_digest:
                return True
            return any(config_owned_by(child['digest'], config_digest, visited)
                       for child in document.get('manifests', []))

        matches = any(
            tag in entry.get('RepoTags', [])
            and config_owned_by(identity, 'sha256:' + hashlib.sha256(
                archive.extractfile(entry['Config']).read()).hexdigest())
            for entry in manifest
        )
except (OSError, tarfile.TarError, ValueError, KeyError, TypeError, AttributeError):
    matches = False
sys.exit(0 if matches else 1)
PYTHON
}

# Publish only a completed Docker save; an interrupted attempt is never reusable.
if (( retain )) && ! archive_matches_image; then
    partial=$(mktemp "$archive.partial.XXXXXX")
    trap 'rm -f "$partial"' EXIT
    docker save --output "$partial" "$image"
    [[ -s "$partial" ]] || { echo "Docker saved an empty runtime archive: $image" >&2; exit 1; }
    # Verify the newly saved archive too; a changed tag or malformed save must
    # not replace a checkpoint that another run may still need.
    previous_archive="$archive"
    archive="$partial"
    if ! archive_matches_image; then
        echo "Docker archive does not match runtime image: $image" >&2
        exit 1
    fi
    archive="$previous_archive"
    mv "$partial" "$archive"
fi
