#!/usr/bin/env bash
# Replays a fixture against the running game over the control port, then takes
# the side-by-side capture with compare.sh. A fixture is JSON lines under
# dev/fixtures/: control commands (`{"type":"load_scene","name":"START"}`,
# `{"type":"input","key":"KeyW","action":"tap"}`, teleport, set_fidelity) and
# `{"sleep": 0.5}`. The same fixture rerun gives the same frame, so a capture is
# evidence another agent can reproduce, not a one-off screenshot.
# Usage: replay.sh <fixture.jsonl> <label>   (both games running: just run-native / just retro)
set -euo pipefail
cd "$(dirname "$0")/../.."
fixture=${1:?usage: replay.sh <fixture.jsonl> <label>}
label=${2:?usage: replay.sh <fixture.jsonl> <label>}
addr=${CYRUS_CONTROL_ADDR:-127.0.0.1:7117}

python3 -I - "$addr" "$fixture" <<'PY'
import json, socket, sys, time
host, port = sys.argv[1].rsplit(":", 1)
s = socket.create_connection((host, int(port)), timeout=30); f = s.makefile("rw")
for n, line in enumerate(open(sys.argv[2]), 1):
    line = line.strip()
    if not line or line.startswith("#"): continue
    step = json.loads(line)
    if "sleep" in step:
        time.sleep(step["sleep"]); continue
    f.write(json.dumps({"id": n, "cmd": step}) + "\n"); f.flush()
    r = json.loads(f.readline())
    assert r.get("ok"), f"{sys.argv[2]}:{n}: {r}"
PY
exec dev/scripts/compare.sh "$label"
