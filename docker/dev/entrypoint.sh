#!/bin/bash
set -euo pipefail
export DISPLAY=:1
export WGPU_BACKEND=vulkan
Xvfb :1 -screen 0 "${SCREEN:-1280x800x24}" -nolisten tcp &
sleep 1
x11vnc -display :1 -nopw -forever -shared -quiet -bg -rfbport 5900
websockify --web /usr/share/novnc 6080 localhost:5900 &
exec "$@"
