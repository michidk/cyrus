#!/usr/bin/env python3
"""Trace the selected player through the control port, one JSON line per game-clock step.

Connects to a running cyrus (CYRUS_CONTROL_ADDR), retries until `state` shows InGame, scene_ready
and loaded scripts, then runs the placement steps and writes one line per change of the game clock:
game seconds, frame, position (map units), animation group, health and the falling, queued, sliding
and living-state bytes. Output goes to --out (keep it under the workspace, never in a tracked path).

    native-trace.py --out run.jsonl --drop 200 --drop 400 --drop 700 --origin 40816,-744,41024 [--seconds 8]
    native-trace.py --out run.jsonl --place 23847,-304,55082 --key Space:2:3 --seconds 10

--drop H places the player H units above --origin and traces --seconds of game time after each;
--place X,Y,Z does one placement; --key NAME:START:END (repeatable) holds a Bevy key code between those game
seconds after each placement. A line's "ev" is "placed" for the sample taken at placement.

Cheats persist in the config directory (CYRUS_CONFIG_DIR) between runs, so a cheat left on by one
run, such as `drevil`, silently changes the next one. This script therefore sets every cheat the
catalog reports non-zero back to 0 before it starts (except task, animation and nodemarker, which
SYSTEM.INI seeds at startup and which must stay as they are) (it prints what it reset to stderr), then applies
each `--cheat NAME=VALUE` you give; pass --keep-cheats to leave the stored values alone.

    native-trace.py --out run.jsonl --cheat magiccarpet=1 --place 40816,-1744,41024 --seconds 6
"""
import argparse
import json
import os
import socket
import sys
import time


def call(addr, cmd, timeout=120):
    host, port = addr.rsplit(":", 1)
    with socket.create_connection((host, int(port)), timeout=timeout) as s:
        s.sendall((json.dumps({"id": 1, "cmd": cmd}) + "\n").encode())
        data = b""
        while not data.endswith(b"\n"):
            chunk = s.recv(1 << 22)
            if not chunk:
                break
            data += chunk
    reply = json.loads(data)
    if not reply.get("ok", True):
        raise RuntimeError(reply.get("error"))
    return reply.get("result", reply)


def wait_live(addr, patience):
    deadline = time.time() + patience
    while time.time() < deadline:
        try:
            s = call(addr, {"type": "state"})
            scripts = s.get("scripts") or {}
            if s.get("state") == "InGame" and s.get("scene_ready") and scripts.get("state") == "Loaded":
                return
        except (OSError, RuntimeError, ValueError):
            pass
        time.sleep(3)
    sys.exit("native-trace: game did not reach InGame with scripts loaded")


def player_index(addr):
    offset = 0
    while True:
        page = call(addr, {"type": "scripts", "offset": offset, "limit": 64})["actors"]
        for i, actor in enumerate(page):
            if actor.get("source") == "player" and actor.get("name") == "CYRUS":
                return offset + i
        if len(page) < 64:
            sys.exit("native-trace: selected player not found")
        offset += 64


def sample(addr, index, ev):
    actor = call(addr, {"type": "scripts", "offset": index, "limit": 1})["actors"][0]
    state = call(addr, {"type": "state"})
    control = actor.get("control") or {}
    attrs = actor.get("attributes") or {}
    # The attribute table is a byte list; attribute 3 is health.
    health = attrs[3] if isinstance(attrs, list) and len(attrs) > 3 else None
    pose = actor["spatial"]["effective"]
    return {
        "ev": ev,
        "g": state["clock"]["game_seconds"],
        "frame": state["frame"],
        "pos": [v / 256 for v in pose["position"]],
        "group": (actor.get("animation") or {}).get("current_group"),
        "health": health,
        "falling": control.get("falling_state"),
        "queued": control.get("queued_motion"),
        "sliding": control.get("sliding_state"),
        "living": control.get("living_state"),
        "error": (state.get("scripts") or {}).get("error"),
    }


def main():
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--addr", default=os.environ.get("CYRUS_CONTROL_ADDR", "127.0.0.1:7167"))
    p.add_argument("--out", required=True)
    p.add_argument("--origin", default="40816,-744,41024")
    p.add_argument("--drop", type=int, action="append", default=[])
    p.add_argument("--place", action="append", default=[])
    p.add_argument("--heading", type=int, default=0)
    p.add_argument("--key", action="append", default=[])
    p.add_argument("--cheat", action="append", default=[], metavar="NAME=VALUE")
    p.add_argument("--keep-cheats", action="store_true")
    p.add_argument("--seconds", type=float, default=8.0)
    p.add_argument("--patience", type=float, default=420)
    a = p.parse_args()
    wait_live(a.addr, a.patience)
    index = player_index(a.addr)
    if not a.keep_cheats:
        for entry in call(a.addr, {"type": "state"}).get("cheats", {}).get("entries", []):
            # task, animation and nodemarker are seeded from SYSTEM.INI at startup (task and
            # animation are 1 by design; zeroing them stops the animation system), so they stay.
            if entry.get("value") and entry.get("effect") not in ("list", "save") and entry.get("name") not in ("task", "animation", "nodemarker"):
                print(f"native-trace: reset persisted cheat {entry['name']}={entry['value']}", file=sys.stderr)
                call(a.addr, {"type": "cheat", "name": entry["name"], "value": 0})
    for spec in a.cheat:
        name, value = spec.split("=")
        call(a.addr, {"type": "cheat", "name": name, "value": int(value)})
    origin = [int(v) for v in a.origin.split(",")]
    targets = [[origin[0], origin[1] - h, origin[2]] for h in a.drop]
    targets += [[int(v) for v in t.split(",")] for t in a.place]
    keys = []
    for spec in a.key:
        name, start, end = spec.split(":")
        keys.append([name, float(start), float(end), False, False])
    with open(a.out, "w") as out:
        for target in targets:
            call(a.addr, {"type": "place_player", "x": target[0], "y": target[1], "z": target[2], "heading": a.heading})
            call(a.addr, {"type": "reset_camera"})
            first = sample(a.addr, index, "placed")
            first["target"] = target
            out.write(json.dumps(first) + "\n")
            t0, last = first["g"], first["g"]
            for k in keys:
                k[3] = k[4] = False
            stall = time.time()
            while last < t0 + a.seconds:
                row = sample(a.addr, index, "s")
                if row["g"] > last:
                    last = row["g"]
                    stall = time.time()
                    out.write(json.dumps(row) + "\n")
                    out.flush()
                elif time.time() - stall > 180:
                    sys.exit("native-trace: game clock stalled 180 s")
                for k in keys:
                    if not k[3] and last >= t0 + k[1]:
                        call(a.addr, {"type": "input", "key": k[0], "action": "press"})
                        k[3] = True
                    if k[3] and not k[4] and last >= t0 + k[2]:
                        call(a.addr, {"type": "input", "key": k[0], "action": "release"})
                        k[4] = True
                time.sleep(0.02)


if __name__ == "__main__":
    main()
