#!/usr/bin/env bash
# Records the original's audio mixer output while replaying a key timeline, in one lock hold.
# DOSBox-X's own WAV recorder (Ctrl+Alt+W) records the mixer output in emulated time, so the file is
# independent of how slowly the container emulates; the inner script logs the game's loop frame
# at the start, at each key event and at the stop so audio positions map to the state reader.
# Usage: original-audio-capture.sh <events.json> <out-dir> [seconds]
# Optional: CYRUS_CAPTURE_STARTUP=1 arms recording at a DOS prompt before launching
#           RGFX.EXE and stops at the first positive world-loop frame (or seconds).
#           Use an unmodified final-version run for startup-order comparisons.
# Optional: CYRUS_CAPTURE_HOOK (a python script copied to the container and run with the state
#           reader when the recorder starts (arguments: reader, CYRUS_HOOK_DELAY, then CYRUS_HOOK_ARGS), for timed placement; CYRUS_HOOK_ARGS are its arguments),
#           CYRUS_SHOT_INTERVAL (seconds between framebuffer screenshots, written to
#           <out>/shots and listed in the timeline with the wall times they were taken at).
# Optional: CYRUS_CLOCK_HELPER, a python script run in the container from the recorder start
#           that logs "wall frame ..." lines to <out>/clock.log (kept outside the repository).
# Optional: CYRUS_FRAME_WAIT, seconds to wait for the world loop before recording anyway
#           (use a small value for captures outside a world, such as the intro movie).
# Optional: CYRUS_DOSBOX_AUTOEXEC, newline-separated DOSBox-X commands inserted into the
#           container's autoexec after the drives are mounted and before the game starts (for example `mixer cdaudio 0` to mute the
#           CD music channel, or `mixer >c:\\mixer.txt` to list the channels into the run copy).
# Environment: CYRUS_RETRO_RUN = a private copy of the install tree (never the shared install);
#              CYRUS_RETRO_GOG = the directory mounted read-only at /gog; the run copy's relative
#              symlinks resolve through it, and it must hold extracted/app/game.gog.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
events=$(realpath "${1:?events.json required}")
out=$(realpath -m "${2:?output directory required}")
seconds=${3:-40}
run=${CYRUS_RETRO_RUN:?set CYRUS_RETRO_RUN to a private install copy}
gog=${CYRUS_RETRO_GOG:?set CYRUS_RETRO_GOG to the directory holding extracted/app/game.gog}
name=audio-capture-$$
conf_mount=()
# Configure the host chord even when reusing an image built before this default.
dosbox_host_args=(-set "dosbox hostkey=ctrlalt")
mkdir -p "$out"
exec 9>/tmp/cyrus-live.lock
flock 9
docker image inspect cyrus-retro:dev >/dev/null 2>&1 || docker build -q -t cyrus-retro:dev "$repo/dev/docker/retro" >&2
if [ -n "${CYRUS_DOSBOX_AUTOEXEC:-}" ]; then
    conf="$run/dosbox-x.autoexec.conf"
    docker run --rm --entrypoint cat cyrus-retro:dev /etc/cyrus/dosbox-x.conf > "$conf"
    python3 - "$conf" <<'PY'
import os, sys
path = sys.argv[1]
text = open(path).read()
text = text.replace("\nC:\n", "\n" + os.environ["CYRUS_DOSBOX_AUTOEXEC"] + "\nC:\n", 1)
open(path, "w").write(text)
PY
    conf_mount=(-v "$conf:/etc/cyrus/dosbox-x.conf:ro")
fi
if [ "${CYRUS_CAPTURE_STARTUP:-0}" = 1 ]; then
    conf="$out/dosbox-x.startup.conf"
    python3 - "$repo/dev/docker/retro/dosbox-x.conf" "$conf" <<'PYCONF'
import os, sys
from pathlib import Path
text = Path(sys.argv[1]).read_text()
text = text.replace("rgfx.exe\nexit", "echo ready > C:\\capture\\START.OK")
if os.environ.get("CYRUS_DOSBOX_AUTOEXEC"):
    text = text.replace("\nC:\n", "\n" + os.environ["CYRUS_DOSBOX_AUTOEXEC"] + "\nC:\n", 1)
Path(sys.argv[2]).write_text(text)
PYCONF
    conf_mount=(-v "$conf:/etc/cyrus/dosbox-x.conf:ro")
fi
docker rm -f "$name" >/dev/null 2>&1 || true
mkdir -p "$run/capture"
rm -f "$run"/capture/*.wav "$run/capture/START.OK"
docker run --rm --entrypoint find -v "$run:/game" cyrus-retro:dev /game/Redguard/maps -iname '*.tsg' -delete
docker run -d --rm --name "$name" --memory 2g --memory-swap 2g --cap-add SYS_PTRACE \
    -v "$run:/game" -v "$gog:/gog:ro" -v "$gog/extracted/app/game.gog:/game/game.gog:ro" \
    "${conf_mount[@]}" cyrus-retro:dev "${dosbox_host_args[@]}" >/dev/null
cleanup() {
    docker rm -f "$name" >/dev/null 2>&1 || true
    [ -z "${conf:-}" ] || rm -f "$conf"
}
trap cleanup EXIT
docker cp "$repo/dev/scripts/original-capture-inner.py" "$name:/tmp/inner.py"
docker cp "$events" "$name:/tmp/events.json"
[ -z "${CYRUS_CLOCK_HELPER:-}" ] || docker cp "$CYRUS_CLOCK_HELPER" "$name:/tmp/clock.py"
[ -z "${CYRUS_CAPTURE_HOOK:-}" ] || docker cp "$CYRUS_CAPTURE_HOOK" "$name:/tmp/hook.py"
docker cp "${CYRUS_STATE_READER:?set CYRUS_STATE_READER to original-state.py}" "$name:/tmp/original-state.py"
docker exec -e CYRUS_CAPTURE_STARTUP="${CYRUS_CAPTURE_STARTUP:-0}" -e CYRUS_HOOK_ARGS="${CYRUS_HOOK_ARGS:-}" -e CYRUS_FRAME_WAIT="${CYRUS_FRAME_WAIT:-150}" -e CYRUS_HOOK_DELAY="${CYRUS_HOOK_DELAY:-0}" -e CYRUS_SHOT_INTERVAL="${CYRUS_SHOT_INTERVAL:-0}" "$name" python3 /tmp/inner.py /tmp/original-state.py /tmp/events.json "$seconds" > "$out/timeline.json"
wav=$(ls -t "$run"/capture/*.wav | head -1)
cp "$wav" "$out/audio.wav"
[ ! -f "$run/capture/clock.log" ] || cp "$run/capture/clock.log" "$out/clock.log"
if [ -d "$run/capture/shots" ]; then rm -rf "$out/shots"; mv "$run/capture/shots" "$out/shots"; fi
rm -f "$run"/capture/*.wav "$run/capture/START.OK"
echo "captured $(stat -c %s "$out/audio.wav") bytes of audio into $out"
