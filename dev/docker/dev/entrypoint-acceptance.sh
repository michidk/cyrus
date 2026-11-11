set -euo pipefail
export DISPLAY=:1 WGPU_BACKEND=vulkan
Xvfb :1 -screen 0 "${SCREEN:-640x480x24}" -nolisten tcp &
sleep 1
exec "$@"
