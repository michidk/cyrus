#!/usr/bin/env bash
# Agent-loop smoke: starts the game headless, drives it through cyrus-mcp over stdio,
# checks that state/input/fidelity/quit round-trip. Needs an install; built binaries are used as-is.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${CYRUS_BIN:=target/debug/cyrus}" "${MCP_BIN:=target/debug/cyrus-mcp}"
[ -x "$CYRUS_BIN" ] && [ -x "$MCP_BIN" ] || { echo "build first: cargo build" >&2; exit 1; }
log=$(mktemp)
"$CYRUS_BIN" --headless --scene START >"$log" 2>&1 &
game=$!
trap 'kill $game 2>/dev/null || true' EXIT
for _ in $(seq 1 60); do grep -q 'control: listening' "$log" && break; sleep 1; done
grep -q 'control: listening' "$log" || { echo "game did not start"; tail -20 "$log"; exit 1; }
sleep 3
python3 -I - "$MCP_BIN" <<'PY'
import json, subprocess, sys
p = subprocess.Popen([sys.argv[1]], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
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
before = call("state")["player"]["position"]
call("input", {"key":"ArrowUp","action":"press"})
import time; time.sleep(0.5)
call("input", {"key":"ArrowUp","action":"release"})
after = call("state")["player"]["position"]
assert before != after, "faithful controller did not move the player"
assert call("set_fidelity", {"area":"camera","mode":"modern"})["camera"] == "modern"
call("quit")
print("smoke-mcp: ok", before, "->", after)
PY
wait $game || true
grep -q panicked "$log" && { echo "game panicked:"; grep -A3 panicked "$log"; exit 1; }
echo "smoke-mcp: green"
