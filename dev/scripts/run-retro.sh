#!/usr/bin/env bash
# Original game under DOSBox-X behind the VibePod preview "cyrus-retro".
# Needs the extracted GOG app directory at magic/gog/extracted/app (see docs/SETUP.md).
set -euo pipefail
cd "$(dirname "$0")/../.."
if [ "${1:-}" = --fixture ]; then
    exec node dev/scripts/matched-capture.mjs "${2:?fixture required}" "${3:?capture label required}" --original-only
fi
src=magic/gog/extracted/app
run=magic/retro-run
[ -d "$src/Redguard" ] || { echo "extract the GOG installer to $src first" >&2; exit 1; }
if [ ! -d "$run/Redguard" ]; then
    rsync -a --exclude DOSBOX --exclude __support --exclude webcache.zip --exclude Manual.pdf --exclude '*.dll' "$src/" "$run/"
    cp "$src/DOSBOX/glide2x_emu.ovl" "$run/Redguard/glide2x.ovl"
fi
# RETRO_PRISTINE=1: the install's own SYSTEM.INI and an empty SAVEGAME, so the original starts
# as a player sees it (title, book menu, Introduction). The default copy keeps the debug settings.
pristine=()
if [ "${RETRO_PRISTINE:-}" = 1 ]; then
    mkdir -p magic/retro-pristine/SAVEGAME
    [ -f magic/retro-pristine/SYSTEM.INI ] || cp "$src/Redguard/SYSTEM.INI" magic/retro-pristine/SYSTEM.INI
    pristine=(-v "$PWD/magic/retro-pristine/SYSTEM.INI:/game/Redguard/SYSTEM.INI"
        -v "$PWD/magic/retro-pristine/SAVEGAME:/game/Redguard/SAVEGAME")
fi
# Always build: the layer cache makes it quick, and a changed Dockerfile is never missed.
docker build -q -t cyrus-retro:dev dev/docker/retro >/dev/null
docker rm -f cyrus-retro >/dev/null 2>&1 || true
vibepod-preview run -port 36080 -title "Redguard (original, DOSBox-X)" cyrus-retro -- \
    docker run --rm --name cyrus-retro -p 127.0.0.1:36080:6080 -v "$PWD/$run:/game" -v "$PWD/$src/game.gog:/game/game.gog:ro" "${pristine[@]}" cyrus-retro:dev
