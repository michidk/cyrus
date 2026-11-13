#!/usr/bin/env python3
"""Exercise screenshot acknowledgement in the actual original-game runtime image.

Usage: python3 dev/scripts/test-retro-observer-runtime.py [IMAGE]
Build dev/docker/retro first. Requires Docker, but no game data or preview.
"""
import subprocess
import sys

IMAGE = sys.argv[1] if len(sys.argv) > 1 else 'cyrus-retro:dev'
PROBE = r'''set -euo pipefail
export DISPLAY=:97
Xvfb "$DISPLAY" -screen 0 640x480x24 -nolisten tcp &
xpid=$!
trap 'kill "$xpid"; rm -f /tmp/observer.png' EXIT
for attempt in $(seq 1 100); do
    if xdpyinfo >/dev/null 2>&1; then break; fi
    sleep .05
done
xdpyinfo >/dev/null
scrot -o /tmp/observer.png
python3 - <<'INNER'
from pathlib import Path
from PIL import Image, ImageChops, ImageStat
path = Path('/tmp/observer.png')
with Image.open(path) as source:
    source.load()
    assert source.size == (640, 480), source.size
    frame = source.convert('RGB')
# Readiness matching uses decoded pixels, cropped differences and channel means.
reference = frame.crop((20, 20, 60, 60))
assert ImageStat.Stat(ImageChops.difference(reference, reference.copy())).mean == [0, 0, 0]
changed = Image.new('RGB', reference.size, (255, 0, 0))
assert max(ImageStat.Stat(ImageChops.difference(reference, changed)).mean) == 255
try:
    Image.open(path.with_name('missing.png'))
except FileNotFoundError:
    pass
else:
    raise AssertionError('missing screenshot was admitted')
path.write_bytes(b'invalid png')
try:
    Image.open(path).load()
except OSError:
    pass
else:
    raise AssertionError('corrupt screenshot was admitted')
print('retro observer: actual 640x480 capture decoded; match, mismatch, missing and corrupt-image controls passed')
INNER
'''
subprocess.run(['docker', 'run', '--rm', '-i', '--entrypoint', 'bash', IMAGE, '-s'],
               input=PROBE, text=True, check=True, timeout=60)
