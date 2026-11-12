#!/usr/bin/env bash
# Same guest configuration as the interactive original, without preview services.
set -euo pipefail
export DISPLAY=:1
# Dummy remains the unattended default; explicit monitor captures use PulseAudio.
export SDL_AUDIODRIVER=${SDL_AUDIODRIVER:-dummy}
framebuffer_dir="${CYRUS_CONFIG_DIR:-/config}"
mkdir -p "$framebuffer_dir"
Xvfb :1 -screen 0 "${SCREEN:-1024x768x24}" -nolisten tcp -fbdir "$framebuffer_dir" &
sleep 1
cd /game
[ -s game.cue ] || cp game.ins game.cue
if [[ -n "${CYRUS_FRAME_CAPTURE_DIR:-}" ]]; then
    export LD_PRELOAD="${LD_PRELOAD:+$LD_PRELOAD:}/usr/local/lib/sdl-frame-capture.so"
fi
if [[ -n "${CYRUS_MIXER_CAPTURE_DIR:-}" ]]; then
    export LD_PRELOAD="${LD_PRELOAD:+$LD_PRELOAD:}/usr/local/lib/sdl-mixer-capture.so"
fi
exec dosbox-x -conf /etc/cyrus/dosbox-x.conf "$@"
