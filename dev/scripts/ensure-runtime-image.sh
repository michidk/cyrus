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
# Keep diagnostics beside the host-retained archive, never in the disposable
# daemon. The receipt is evidence, not an admission flag: always recheck Docker.
mkdir -p "$(dirname "$archive")"
exec 8>"$archive.restore.lock"
flock 8
restore_log="$archive.restore.log"
restore_state="$archive.restore-state.json"
daemon_identity=unknown
record_state() {
    python3 - "$restore_state" "$restore_log" "$image" "$1" "$2" "${3:-0}" "$daemon_identity" <<'PYTHON'
import datetime
import json
import os
import sys
import tempfile

path, log, image, stage, status, code, daemon = sys.argv[1:]
state = dict(time=datetime.datetime.now(datetime.timezone.utc).isoformat(),
             image=image, stage=stage, status=status, exit_code=int(code), daemon=daemon)
with open(log, 'a') as output:
    output.write(json.dumps(state) + '\n')
    output.flush()
    os.fsync(output.fileno())
fd, temporary = tempfile.mkstemp(dir=os.path.dirname(path), prefix='.restore-state-')
with os.fdopen(fd, 'w') as output:
    json.dump(state, output)
    output.write('\n')
    output.flush()
    os.fsync(output.fileno())
os.replace(temporary, path)
directory = os.open(os.path.dirname(path), os.O_RDONLY | os.O_DIRECTORY)
try:
    os.fsync(directory)
finally:
    os.close(directory)
PYTHON
}
run_step() {
    local stage="$1"
    shift
    record_state "$stage" running
    printf '%s\n' "$*" >> "$restore_log"
    if "$@" 2>&1 | tee -a "$restore_log" >&2; then
        record_state "$stage" succeeded
    else
        local code=$?
        record_state "$stage" failed "$code"
        return "$code"
    fi
}
previous_state=$(cat "$restore_state" 2>/dev/null || true)
# Record daemon identity on every invocation, including recovery after a reset.
# Failure here reports unavailable Docker rather than attempting to start it.
run_step availability docker info --format '{{json .ID}} {{json .ServerVersion}} {{json .Driver}}'
daemon_identity=$(docker info --format '{{.ID}}')
if [ -f "$restore_state" ]; then
    python3 - "$previous_state" "$daemon_identity" <<'PYTHON'
import json
import sys
state = json.loads(sys.argv[1]) if sys.argv[1] else {}
previous = state.get('daemon', 'unknown')
if previous not in ('unknown', sys.argv[2]):
    print('runtime daemon changed; rechecking image and retained archive', file=sys.stderr)
PYTHON
fi
record_state inspection running

if ! docker image inspect "$image" >/dev/null 2>&1; then
    if [[ ! -s "$archive" ]]; then
        record_state load failed 1
        echo "runtime image $image is unavailable and archive is missing or empty: $archive" >&2
        exit 1
    fi
    # A failed client request can still leave reusable image metadata/content.
    # Inspect that state before deciding whether the whole import failed.
    load_code=0
    run_step load docker load --input "$archive" || load_code=$?
    if ! docker image inspect "$image" >/dev/null 2>&1; then
        if (( load_code == 0 )); then load_code=1; fi
        record_state load failed "$load_code"
        echo "runtime archive $archive did not restore requested image $image; diagnostics: $restore_log" >&2
        exit 1
    fi
fi
# Preparing a disposable container resumes retained layers and checks the rootfs.
# No game/config mounts, daemon changes, deletion or automatic reload loop.
run_step prepare docker run --rm --pull never --network none --read-only --entrypoint /bin/true "$image"
if (( ! retain )); then
    record_state complete succeeded
    exit 0
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
    run_step save docker save --output "$partial" "$image"
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

record_state complete succeeded
