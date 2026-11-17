#!/usr/bin/env bash
# Use an existing writable run; never copy or modify the installed game.
set -euo pipefail
if (( $# < 4 )); then
    echo 'usage: original-rope-contact.sh RUN READER.py PLACE.py OUTPUT [driver options...]' >&2
    exit 2
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
run="$(realpath "$1")"
reader="$(realpath "$2")"
placement="$(realpath "$3")"
output="$(realpath -m "$4")"
shift 4
for path in "$run" "$reader" "$placement" "$output"; do
    case "$path" in
        /home/vibepod/workspaces/*) ;;
        *) echo "contact run paths must be inside /home/vibepod/workspaces: $path" >&2; exit 2 ;;
    esac
done
install_root="$(realpath -e "${CYRUS_RETRO_INSTALL_ROOT:-$root/magic/gog}")"
case "$run/" in
    "$install_root/"*) echo 'the install is read-only; supply a prepared run directory' >&2; exit 2 ;;
esac
case "$output/" in
    "$install_root/"*) echo 'output must be outside the read-only install' >&2; exit 2 ;;
esac
if [[ ! -f "$reader" || ! -f "$placement" ]]; then
    echo 'requires existing reader and placement Python files' >&2
    exit 2
fi
world="$run/Redguard/WORLD.INI"
system="$run/Redguard/SYSTEM.INI"
installed_system="${CYRUS_RETRO_SYSTEM_INI:-$root/redguard/SYSTEM.INI}"
if [[ ! -f "$system" || -L "$system" || ! -f "$installed_system" ]]; then
    echo 'requires real writable SYSTEM.INI and installed SYSTEM.INI' >&2
    exit 2
fi
if [[ ! -f "$world" || -L "$world" || ! -f "$run/game.ins" ]]; then
    echo 'requires a prepared writable run with a real WORLD.INI and game.ins' >&2
    exit 2
fi
# This driver observes a world, not the final-version book startup path.
python3 - "$system" <<'PYADMISSION'
import configparser
from pathlib import Path
import sys

path = Path(sys.argv[1])
config = configparser.ConfigParser(interpolation=None)
try:
    config.read_string(path.read_text())
    admitted = config.getint('debug', 'final_version') == 0
except (OSError, UnicodeError, configparser.Error, ValueError):
    admitted = False
if not admitted:
    print(f'original contact: {path} [debug] final_version must be 0 for direct-world capture; '
          'use the startup capture driver for final-version book entry', file=sys.stderr)
    sys.exit(2)
PYADMISSION
# Fresh GOG runs do not contain the DOSBox Glide overlay beside RGFX.EXE.
# Supply it read-only from the install, retaining an explicitly prepared run overlay.
overlay_mount=()
if [[ ! -s "$run/Redguard/glide2x.ovl" ]]; then
    overlay="$install_root/extracted/app/DOSBOX/glide2x_emu.ovl"
    if [[ ! -s "$overlay" ]]; then
        echo "original capture: Glide emulation overlay unavailable: $overlay" >&2
        exit 2
    fi
    overlay_mount=(--mount "type=bind,src=$overlay,dst=/game/Redguard/glide2x.ovl,readonly")
fi
# Recover before acquiring the live lock; image restoration is not a game run.
image="${CYRUS_RETRO_IMAGE:-}"
if [[ -n "${CYRUS_RETRO_IMAGE_ARCHIVE:-}" ]]; then
    image="${image:-cyrus-retro-contact:dev}"
    "$root/dev/scripts/ensure-runtime-image.sh" "$image" "$CYRUS_RETRO_IMAGE_ARCHIVE"
elif [[ -z "$image" ]]; then
    image=$("$root/dev/scripts/ensure-retro-capture-runtime.sh")
fi
mkdir -p "$output"
exec 9>/tmp/cyrus-live.lock
if flock -w 30 9; then
    :
else
    status=$?
    echo 'original capture: live-run lock unavailable after 30s: /tmp/cyrus-live.lock' >&2
    exit "$status"
fi
name="cyrus-rope-contact-$$"
cp -a "$world" "$output/WORLD.before"
cp -a "$system" "$output/SYSTEM.before"
created=0
cleanup() {
    if (( created )); then
        docker inspect "$name" > "$output/container.json" 2>/dev/null || true
        docker logs "$name" > "$output/container.log" 2>&1 || true
        docker rm -f "$name" >/dev/null 2>&1 || true
    fi
    cp -a "$output/WORLD.before" "$world"
    cp -a "$output/SYSTEM.before" "$system"
}
trap cleanup EXIT
# Preserve direct-world/developer admission, but use installed text suppression.
python3 - "$installed_system" "$system" <<'PYCONFIG'
import configparser
from pathlib import Path
import re
import sys
source = configparser.ConfigParser(interpolation=None)
source.read(sys.argv[1])
path = Path(sys.argv[2])
text = path.read_text()
for key in ('disable_text', 'disable_debug_text'):
    value = source['system'][key]
    text, count = re.subn(rf'(?im)^\s*{key}\s*=.*$',
                          lambda _: f'{key}={value}', text)
    if count != 1:
        raise SystemExit(f'{path}: expected one {key} setting, found {count}')
path.write_text(text)
PYCONFIG
sed -i 's/^start_world=.*/start_world=2/' "$world"
mkdir -p "$output/config" "$output/audio" "$output/runtime"
cp -a "$root/dev/scripts/original-rope-contact.py" "$output/runtime/driver.py"
docker run -d --name "$name" --cap-add SYS_PTRACE -e DISPLAY=:1 \
    -e CYRUS_CONFIG_DIR=/config \
    --mount "type=bind,src=$run,dst=/game" \
    --mount "type=bind,src=$install_root,dst=/gog,readonly" \
    --mount "type=bind,src=$install_root/extracted/app/game.gog,dst=/game/game.gog,readonly" \
    --mount "type=bind,src=$output/config,dst=/config" \
    --mount "type=bind,src=$output/audio,dst=/capture" \
    --mount "type=bind,src=$reader,dst=/tmp/state.py,readonly" \
    --mount "type=bind,src=$placement,dst=/tmp/place.py,readonly" \
    --mount "type=bind,src=$output/runtime/driver.py,dst=/tmp/driver.py,readonly" \
    "${overlay_mount[@]}" \
    "$image" -set "dosbox hostkey=ctrlalt" -set "dosbox captures=/capture" >/dev/null
created=1
docker exec "$name" python3 /tmp/driver.py --reader /tmp/state.py --progress /config/rope-readiness.json \
    "$@" --place python3 /tmp/place.py >> "$output/contact.jsonl" 2>> "$output/contact.stderr"
