#!/usr/bin/env bash
# Run a command with a real-time audio sink: a PulseAudio null sink that ALSA's default device uses.
# ALSA's own `null` device drains faster than real time, which cuts every voiced line short and
# makes scripted scenes (waits on dialogue) run about 30% quicker than the original; this sink
# consumes samples at the sample rate, as a sound card does.
#   docker run ... cyrus-dev:dev paced-audio.sh /repo/target/debug/cyrus --skip-startup
set -e
mkdir -p /run/pulse
cat > /tmp/cyrus.pa <<'PA'
load-module module-native-protocol-unix auth-anonymous=1 socket=/run/pulse/native
load-module module-null-sink sink_name=cyrus
set-default-sink cyrus
PA
pulseaudio --system -n -F /tmp/cyrus.pa --daemonize=yes --disallow-exit --exit-idle-time=-1 \
    --disable-shm=yes --log-level=error 2>/tmp/pulse.log
sleep 2
export PULSE_SERVER=unix:/run/pulse/native
printf 'pcm.!default { type pulse }\nctl.!default { type pulse }\n' > /etc/asound.conf
exec "$@"
