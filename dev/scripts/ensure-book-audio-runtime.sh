#!/usr/bin/env bash
# Recover a native book PCM runtime with both software Vulkan and a real mixer.
set -euo pipefail
if (( $# != 2 )); then
    echo 'usage: ensure-book-audio-runtime.sh IMAGE LOCAL_ARCHIVE' >&2
    exit 2
fi
repo=$(cd "$(dirname "$0")/../.." && pwd)
image=$1
archive=$2
exec 8>"$archive.restore.lock"
flock 8
if ! docker image inspect "$image" >/dev/null 2>&1; then
    if [[ -s "$archive" ]]; then
        docker load --input "$archive"
    else
        docker build -t "$image" -f "$repo/dev/docker/dev/Dockerfile.sound-acceptance" "$repo/dev/docker/dev"
    fi
fi
# A retro image has a mixer but no Vulkan; older native images have Vulkan but
# no mixer. Fail before launching the game rather than silently recording null audio.
docker run --rm --entrypoint bash "$image" -ec '
    for tool in python3 Xvfb pulseaudio pactl parec paplay; do command -v "$tool" >/dev/null; done
    test -s /usr/share/vulkan/icd.d/lvp_icd.json
    python3 -c '\''import ctypes, glob; ctypes.CDLL("libvulkan.so.1"); ctypes.CDLL(glob.glob("/usr/lib/*/alsa-lib/libasound_module_pcm_pulse.so")[0])'\''
'
if [[ ! -s "$archive" ]]; then
    temporary="$archive.tmp"
    trap 'rm -f "$temporary"' EXIT
    docker save --output "$temporary" "$image"
    mv "$temporary" "$archive"
fi
