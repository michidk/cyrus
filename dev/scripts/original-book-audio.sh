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
# Different evidence directories must not concurrently drive the same guest/config.
exec 7>"$run/.cyrus-book-audio.lock"
flock -n 7
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
system_ini="$run/Redguard/SYSTEM.INI"
system_mount=()
cleanup() {
    docker logs "$name" > "$out/container.log" 2>&1 || true
    docker rm -f "$name" >/dev/null 2>&1 || true
}
trap cleanup EXIT
# Adaptive cycles let the guest mixer keep up with a real-time sink under host load.
# These are capture options, not changes to the interactive game's configuration.
dosbox_args=(-set "dosbox hostkey=ctrlalt")
if [[ "${CYRUS_BOOK_REALTIME:-0}" == 1 ]]; then
    dosbox_args+=(-set "cpu cycles=max")
fi
if [[ "${CYRUS_BOOK_ISOLATE_MUSIC:-0}" == 1 ]]; then
    [[ ! -L "$system_ini" && -f "$system_ini" &&
       "$(realpath "$run/Redguard")" != "$(realpath -m "$gog/extracted/app/Redguard")" ]] || {
        echo 'music isolation requires a private run with a real SYSTEM.INI' >&2; exit 1;
    }
    cp -a "$system_ini" "$out/runtime/SYSTEM.capture.ini"
    system_mount=(-v "$out/runtime/SYSTEM.capture.ini:/game/Redguard/SYSTEM.INI")
    python3 - "$out/runtime/SYSTEM.capture.ini" <<'PYINI'
from pathlib import Path
import re
import sys
path = Path(sys.argv[1])
text = path.read_bytes()
text, count = re.subn(rb'(?im)^(redbook[ \t]*=[ \t]*)on([ \t]*\r?$)', rb'\g<1>off\2', text)
if count != 1:
    raise SystemExit('music isolation requires exactly one enabled redbook setting')
path.write_bytes(text)
PYINI
fi
docker run -d --name "$name" \
    -e SDL_AUDIODRIVER=pulseaudio -e PULSE_SERVER=unix:/run/pulse/native \
    -e CYRUS_CONFIG_DIR=/evidence/config \
    -v "$run:/game" -v "$gog:/gog:ro" "${system_mount[@]}" \
    -v "$gog/extracted/app/game.gog:/game/game.gog:ro" \
    -v "$out:/evidence" -v "$out/runtime/dosbox.conf:/etc/cyrus/dosbox-x.conf:ro" \
    --entrypoint bash cyrus-retro:dev /evidence/runtime/paced-audio.sh \
    bash /evidence/runtime/entrypoint.sh "${dosbox_args[@]}" > "$out/container.id"
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
