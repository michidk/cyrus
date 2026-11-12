#!/usr/bin/env bash
# Agent-loop smoke: starts the game headless, drives it through cyrus-mcp over stdio,
# checks that state/input/fidelity/quit round-trip. Needs an install. The built binaries are
# started directly so a concurrent cargo job holding the build lock cannot stall the run.
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/scripts/cargo.sh
# The install is read-only (hard rule 2): path, size and mtime of every file before and after.
manifest() { find -L redguard -type f -printf '%p %s %T@\n' | sort | sha256sum; }
before=$(manifest)
cargo build -q -p cyrus -p cyrus-mcp
log=$(mktemp)
timeout 300 "${CARGO_TARGET_DIR:-target}/debug/cyrus" --headless --scene START --control "${CYRUS_CONTROL_ADDR:-127.0.0.1:7117}" >"$log" 2>&1 &
game=$!
trap 'kill $game 2>/dev/null || true' EXIT
for _ in $(seq 1 120); do grep -q 'control: listening' "$log" && break; sleep 1; done
grep -q 'control: listening' "$log" || { echo "game did not start"; tail -20 "$log"; exit 1; }
python3 -I - <<'PY'
import json, os, subprocess, sys, time
p = subprocess.Popen([os.path.join(os.environ.get("CARGO_TARGET_DIR", "target"), "debug", "cyrus-mcp")], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
def rpc(o):
    p.stdin.write(json.dumps(o) + "\n"); p.stdin.flush(); return json.loads(p.stdout.readline())
rpc({"jsonrpc":"2.0","id":0,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"smoke","version":"0"}}})
p.stdin.write(json.dumps({"jsonrpc":"2.0","method":"notifications/initialized"}) + "\n"); p.stdin.flush()
tools = {t["name"] for t in rpc({"jsonrpc":"2.0","id":1,"method":"tools/list"})["result"]["tools"]}
assert {"ping","state","screenshot","teleport","input","set_fidelity","skip_startup","load_scene","cheat","console","quit"} <= tools, tools
def call(n, a={}):
    r = rpc({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":n,"arguments":a}})
    assert "error" not in r, (n, r)
    return json.loads(r["result"]["content"][0]["text"])
def wait_state(predicate, description):
    deadline = time.monotonic() + 120
    latest = call("state")
    while not predicate(latest) and time.monotonic() < deadline:
        time.sleep(0.1)
        latest = call("state")
    assert predicate(latest), (description, latest)
    return latest
initial = wait_state(
    lambda s: s["state"] == "InGame" and s["scene"] == "START"
    and s["scripts"] is not None and s["scripts"]["state"] == "Loaded",
    "START metadata did not load",
)
assert initial["cheats"]["ready"], initial["cheats"]
original_cheats = call("cheat", {"name":"oracle"})["cheats"]
oddjob = next(e["value"] for e in original_cheats["entries"] if e["name"] == "oddjob")
changed = call("cheat", {"name":"oddjob", "value":7})
assert changed["effect"] == "recognized_only", changed
assert next(e["value"] for e in changed["cheats"]["entries"] if e["name"] == "oddjob") == 7
call("cheat", {"name":"oddjob", "value":oddjob})
assert call("cheat", {"name":"oracle"})["cheats"] == original_cheats
console = call("console", {"open":True, "line":"oracle"})
assert console["console"]["open"]
assert any("neversaydie" in line["text"] for line in console["console"]["lines"])
call("console", {"open":False})
assert not call("state")["console"]["open"]
call("load_scene", {"name":"START"})
assert initial["flycam"]["position"] is None, "headless host created a camera"
call("set_fidelity", {"area":"player_controller","mode":"modern"})
call("input", {"key":"KeyW","action":"press"})
held = wait_state(lambda s: s["player_input"]["forward"], "held key was not sampled")
# Export completion admits key sampling. The headless loader has no rendered
# world, so sampling must not start the ordinary simulation or move the player.
assert not held["scene_ready"], held
assert held["scripts"]["ordinary_updates"] == 0, held
call("input", {"key":"KeyW","action":"release"})
final = wait_state(lambda s: not s["player_input"]["forward"], "released key remained held")
assert final["scripts"]["ordinary_updates"] == 0, final
assert initial["player"]["position"] == final["player"]["position"], "input enabled unsupported movement"
assert call("set_fidelity", {"area":"camera","mode":"modern"})["camera"] == "modern"
call("quit")
print("smoke-mcp: headless state/input/fidelity/cheats/quit passed")
PY
wait $game || true
grep -q panicked "$log" && { echo "game panicked:"; grep -A3 panicked "$log"; exit 1; }
[ "$(manifest)" = "$before" ] || { echo "smoke-mcp: the run modified the install"; exit 1; }
echo "smoke-mcp: green"
