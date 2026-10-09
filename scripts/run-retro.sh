#!/usr/bin/env bash
# Original game under DOSBox-X behind the VibePod preview "cyrus-retro".
# Needs the extracted GOG app directory at magic/gog/extracted/app (see docs/SETUP.md).
set -euo pipefail
cd "$(dirname "$0")/.."
src=magic/gog/extracted/app
run=magic/retro-run
[ -d "$src/Redguard" ] || { echo "extract the GOG installer to $src first" >&2; exit 1; }
if [ ! -d "$run/Redguard" ]; then
    rsync -a --exclude DOSBOX --exclude __support --exclude webcache.zip --exclude Manual.pdf --exclude '*.dll' "$src/" "$run/"
    cp "$src/DOSBOX/glide2x_emu.ovl" "$run/Redguard/glide2x.ovl"
fi
docker image inspect cyrus-retro:dev >/dev/null 2>&1 || docker build -t cyrus-retro:dev docker/retro
docker rm -f cyrus-retro >/dev/null 2>&1 || true
vibepod-preview run -port 36080 -title "Redguard (original, DOSBox-X)" cyrus-retro -- \
    docker run --rm --name cyrus-retro -p 127.0.0.1:36080:6080 -v "$PWD/$run:/game" cyrus-retro:dev
