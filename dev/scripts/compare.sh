#!/usr/bin/env bash
# Side-by-side capture: one screenshot from cyrus (control port) and one from the
# original in the retro container (screen grab), both into cyrus-config/compare/.
# Usage: compare.sh <label>   Both games must already be running (just run-native / just retro).
set -euo pipefail
cd "$(dirname "$0")/../.."
label=${1:?usage: compare.sh <label>}
out=cyrus-config/compare
mkdir -p "$out"
addr=${CYRUS_CONTROL_ADDR:-127.0.0.1:7117}

# cyrus: ask the running game to write a PNG.
python3 -I - "$addr" "$out/$label-cyrus.png" <<'PY'
import json, socket, sys, time, os
host, port = sys.argv[1].rsplit(":", 1)
s = socket.create_connection((host, int(port)), timeout=10); f = s.makefile("rw")
f.write(json.dumps({"id": 1, "cmd": {"type": "screenshot", "path": sys.argv[2]}}) + "\n"); f.flush()
r = json.loads(f.readline()); assert r["ok"], r
for _ in range(100):
    if os.path.exists(sys.argv[2]) and os.path.getsize(sys.argv[2]) > 0: break
    time.sleep(0.05)
else: sys.exit("cyrus screenshot did not appear")
PY

# original: grab the retro container's virtual display (DOSBox-X's own capture key does not
# fire under Xvfb without a window manager).
docker exec -e DISPLAY=:1 cyrus-retro scrot -o "/game/capture/$label.png"
docker cp "cyrus-retro:/game/capture/$label.png" "$out/$label-original.png"
echo "$out/$label-cyrus.png"
echo "$out/$label-original.png"

# An accepted capture under baseline/ turns this into a regression check: normalised RMSE
# above the tolerance fails. Accept a new look by copying the capture over the baseline.
baseline=$out/baseline/$label-cyrus.png
if [ -f "$baseline" ]; then
    metric=$(compare -metric RMSE "$baseline" "$out/$label-cyrus.png" null: 2>&1) || [ "$?" -eq 1 ]
    rmse=$(sed 's/.*(\(.*\))/\1/' <<<"$metric")
    echo "baseline rmse $rmse (tolerance ${CYRUS_COMPARE_TOLERANCE:-0.02})"
    awk -v r="$rmse" -v t="${CYRUS_COMPARE_TOLERANCE:-0.02}" 'BEGIN { exit !(r <= t) }' \
        || { echo "compare: $label differs from its baseline"; exit 1; }
fi
