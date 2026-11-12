#!/usr/bin/env bash
# One recoverable original-game monitor capture. Observers stay outside the tracked tree.
set -euo pipefail
if (( $# < 6 )); then
    echo 'usage: original-book-audio.sh RUN GOG OUTPUT IMAGE_ARCHIVE READY.py TRANSITION.py [REFERENCE.wav ...]' >&2
    exit 2
fi
repo=$(cd "$(dirname "$0")/../.." && pwd)
run=$(realpath "$1")
gog=$(realpath "$2")
out=$(realpath -m "$3")
archive=$(realpath "$4")
ready=$(realpath "$5")
driver=$(realpath "$6")
mkdir -p "$out/runtime" "$out/config"
exec 9>"$out/run.lock"
flock -n 9
[[ ! -e "$out/capture/result.json" && ! -e "$out/capture/audio.wav" ]] || {
    echo 'capture exists; preserve evidence and use a new output' >&2; exit 1;
}
"$repo/dev/scripts/ensure-runtime-image.sh" cyrus-retro:dev "$archive"
cp -a "$repo/dev/scripts/paced-mixer-capture.py" "$out/runtime/capture.py"
cp -a "$repo/dev/docker/retro/entrypoint.sh" "$out/runtime/entrypoint.sh"
cp -a "$repo/dev/docker/dev/paced-audio.sh" "$out/runtime/paced-audio.sh"
cp -a "$ready" "$out/runtime/ready.py"
cp -a "$driver" "$out/runtime/transition.py"
cp -a "${CYRUS_DOSBOX_CONFIG:-$repo/dev/docker/retro/dosbox-x.conf}" "$out/runtime/dosbox.conf"
name=book-audio-$$
cleanup() {
    docker logs "$name" > "$out/container.log" 2>&1 || true
    docker rm -f "$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT
docker run -d --name "$name" \
    -e SDL_AUDIODRIVER=pulseaudio -e PULSE_SERVER=unix:/run/pulse/native \
    -e CYRUS_CONFIG_DIR=/evidence/config \
    -v "$run:/game" -v "$gog:/gog:ro" \
    -v "$gog/extracted/app/game.gog:/game/game.gog:ro" \
    -v "$out:/evidence" -v "$out/runtime/dosbox.conf:/etc/cyrus/dosbox-x.conf:ro" \
    --entrypoint bash cyrus-retro:dev /evidence/runtime/paced-audio.sh \
    bash /evidence/runtime/entrypoint.sh > "$out/container.id"
docker exec -e DISPLAY=:1 -e PULSE_SERVER=unix:/run/pulse/native "$name" \
    python3 /evidence/runtime/capture.py \
    --ready-command 'python3 /evidence/runtime/ready.py' \
    --timeout "${CYRUS_BOOK_TIMEOUT:-120}" /evidence/capture \
    python3 /evidence/runtime/transition.py

# A paced monitor is insufficient evidence of uninterrupted emulator playback.
if (( $# > 6 )); then
    references=("${@:7}")
    arguments=()
    for reference in "${references[@]}"; do
        arguments+=(--reference "$(realpath "$reference")")
    done
    "${CYRUS_AUDIO_ANALYSIS_PYTHON:-python3}" "$repo/dev/scripts/book-audio-continuity.py" \
        "$out/capture/audio.wav" "$out/continuity.json" "${arguments[@]}"
fi
