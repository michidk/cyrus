#!/bin/bash
# Expects the GOG app directory mounted at /game (contains Redguard/ and game.ins).
set -euo pipefail
export DISPLAY=:1
Xvfb :1 -screen 0 "${SCREEN:-1024x768x24}" -nolisten tcp &
sleep 1
x11vnc -display :1 -nopw -forever -shared -quiet -bg -rfbport 5900
websockify --web /usr/share/novnc 6080 localhost:5900 &
cd /game
# The game writes saves, so it needs a writable tree; the caller decides whether
# /game is a copy or the real install.
if [ ! -f Redguard/glide2x.ovl ]; then
    cp "$(find / -name glide2x_emu.ovl -print -quit 2>/dev/null || true)" Redguard/glide2x.ovl 2>/dev/null || true
fi
exec dosbox-x -conf /etc/cyrus/dosbox-x.conf "$@"
