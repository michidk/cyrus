#!/usr/bin/env bash
# Agent-loop smoke: starts the game headless, drives it through cyrus-mcp over stdio,
# checks that state/input/fidelity/quit round-trip. Needs an install. The built binaries are
# started directly so a concurrent cargo job holding the build lock cannot stall the run.
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/scripts/cargo.sh
cargo build -q -p cyrus -p cyrus-mcp
log=$(mktemp)
"${CARGO_TARGET_DIR:-target}/debug/cyrus" --headless --scene START >"$log" 2>&1 &
game=$!
trap 'kill $game 2>/dev/null || true' EXIT
for _ in $(seq 1 120); do grep -q 'control: listening' "$log" && break; sleep 1; done
grep -q 'control: listening' "$log" || { echo "game did not start"; tail -20 "$log"; exit 1; }
sleep 3
python3 -I - <<'PY'
import json, os, subprocess, sys
p = subprocess.Popen([os.path.join(os.environ.get("CARGO_TARGET_DIR", "target"), "debug", "cyrus-mcp")], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
def rpc(o):
    p.stdin.write(json.dumps(o) + "\n"); p.stdin.flush(); return json.loads(p.stdout.readline())
rpc({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}})
p.stdin.write(json.dumps({"jsonrpc":"2.0","method":"notifications/initialized"}) + "\n"); p.stdin.flush()
tools = {t["name"] for t in rpc({"jsonrpc":"2.0","id":1,"method":"tools/list"})["result"]["tools"]}
assert {"ping","state","screenshot","teleport","input","set_fidelity","load_scene","quit"} <= tools, tools
def call(n, a={}):
    r = rpc({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":n,"arguments":a}})
    assert "error" not in r, (n, r)
    return json.loads(r["result"]["content"][0]["text"])
initial = call("state")
before = initial["flycam"]["position"]
call("input", {"key":"KeyW","action":"press"})
import time; time.sleep(0.5)
call("input", {"key":"KeyW","action":"release"})
final = call("state")
after = final["flycam"]["position"]
assert before != after, "inspection camera did not move"
assert initial["player"]["position"] == final["player"]["position"], "flycam moved the player"
assert call("set_fidelity", {"area":"camera","mode":"modern"})["camera"] == "modern"
call("quit")
print("smoke-mcp: ok", before, "->", after)
PY
wait $game || true
grep -q panicked "$log" && { echo "game panicked:"; grep -A3 panicked "$log"; exit 1; }
echo "smoke-mcp: green"
