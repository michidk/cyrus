#!/usr/bin/env bash
# JSON fixtures produce matched original/native/browser scene captures.
# Legacy JSONL fixtures replay native control commands and host sleeps, then
# capture both running games without establishing a matched original viewpoint.
# Usage: replay.sh <fixture.json|fixture.jsonl> <label>
set -euo pipefail
cd "$(dirname "$0")/../.."
fixture=${1:?usage: replay.sh <fixture.json|fixture.jsonl> <label>}
label=${2:?usage: replay.sh <fixture.json|fixture.jsonl> <label>}
if [[ "$fixture" == *.json ]]; then
    exec node dev/scripts/matched-capture.mjs "$fixture" "$label"
fi
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
