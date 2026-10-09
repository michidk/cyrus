#!/usr/bin/env bash
# Side-by-side capture: one screenshot from cyrus (control port) and one from the
# original in the retro container (DOSBox-X capture key), both into cyrus-config/compare/.
# Usage: compare.sh <label>   Both games must already be running (just run-novnc / just retro).
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

# original: DOSBox-X writes screenshots to its capture directory on Ctrl+F5.
before=$(docker exec cyrus-retro sh -c 'ls /game/capture 2>/dev/null | wc -l' || echo 0)
docker exec -e DISPLAY=:1 cyrus-retro xdotool key ctrl+F5
for _ in $(seq 1 50); do
    sleep 0.1
    after=$(docker exec cyrus-retro sh -c 'ls /game/capture 2>/dev/null | wc -l' || echo 0)
    [ "$after" -gt "$before" ] && break
done
newest=$(docker exec cyrus-retro sh -c 'ls -t /game/capture/*.png 2>/dev/null | head -1')
[ -n "$newest" ] || { echo "no DOSBox-X capture appeared" >&2; exit 1; }
docker cp "cyrus-retro:$newest" "$out/$label-original.png"
echo "$out/$label-cyrus.png"
echo "$out/$label-original.png"
