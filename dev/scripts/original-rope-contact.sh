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
install_root="$(realpath "$root/magic/gog")"
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
if [[ ! -f "$world" || -L "$world" || ! -f "$run/game.ins" ]]; then
    echo 'requires a prepared writable run with a real WORLD.INI and game.ins' >&2
    exit 2
fi
mkdir -p "$output"
exec 9>/tmp/cyrus-live.lock
flock -w 600 9
name="cyrus-rope-contact-$$"
cp -a "$world" "$output/WORLD.before"
created=0
cleanup() {
    if (( created )); then
        docker inspect "$name" > "$output/container.json" 2>/dev/null || true
        docker logs "$name" > "$output/container.log" 2>&1 || true
        docker rm -f "$name" >/dev/null 2>&1 || true
    fi
    cp -a "$output/WORLD.before" "$world"
}
trap cleanup EXIT
sed -i 's/^start_world=.*/start_world=2/' "$world"
mkdir -p "$output/config"
docker run -d --name "$name" --cap-add SYS_PTRACE -e DISPLAY=:1 \
    -e CYRUS_CONFIG_DIR=/config \
    -v "$run:/game" -v "$root/magic/gog:/gog:ro" \
    -v "$root/magic/gog/extracted/app/game.gog:/game/game.gog:ro" \
    -v "$output/config:/config" \
    -v "$reader:/tmp/state.py:ro" -v "$placement:/tmp/place.py:ro" \
    -v "$root/dev/scripts/original-rope-contact.py:/tmp/driver.py:ro" \
    "${CYRUS_RETRO_IMAGE:-cyrus-retro:dev}" -set "dosbox hostkey=ctrlalt" >/dev/null
created=1
docker exec "$name" python3 /tmp/driver.py --reader /tmp/state.py \
    "$@" --place python3 /tmp/place.py > "$output/contact.jsonl" 2> "$output/contact.stderr"
