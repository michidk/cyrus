#!/usr/bin/env bash
# Same guest configuration as the interactive original, without preview services.
set -euo pipefail
export DISPLAY=:1
export SDL_AUDIODRIVER=dummy
Xvfb :1 -screen 0 "${SCREEN:-1024x768x24}" -nolisten tcp -fbdir /config &
sleep 1
cd /game
[ -s game.cue ] || cp game.ins game.cue
exec dosbox-x -conf /etc/cyrus/dosbox-x.conf "$@"
