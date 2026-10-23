#!/usr/bin/env python3
"""Play the START pirate duel through the control port with installed keys only.

Starts a fresh START from the menu (launch cyrus with --skip-startup and no --scene), draws the
sword, then turns toward, approaches and attacks the nearest living pirate until both are
defeated. The `drevil` cheat keeps the player alive, so the bot measures the duel, not the
player's survival. Prints one line per sample and exits 0 once both pirates are gone, 1 on
failure or timeout. Needs a running native cyrus with CYRUS_CONTROL_ADDR set; the default
frame rate of a software-rendered container is enough.

    dev/scripts/start-duel.py [--port 7117] [--limit 900]
"""
import argparse
import json
import math
import socket
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--port", type=int, default=7117)
    parser.add_argument("--limit", type=int, default=900, help="decision steps before giving up")
    args = parser.parse_args()

    def call(cmd):
        with socket.create_connection(("127.0.0.1", args.port), timeout=60) as conn:
            conn.sendall((json.dumps({"id": 1, "cmd": cmd}) + "\n").encode())
            buf = b""
            while not buf.endswith(b"\n"):
                chunk = conn.recv(65536)
                if not chunk:
                    break
                buf += chunk
        reply = json.loads(buf.decode())
        if not reply.get("ok"):
            raise SystemExit(f"control {cmd['type']}: {reply.get('error')}")
        return reply["result"]

    def wait(done, limit=480):
        end = time.time() + limit
        while time.time() < end:
            try:
                state = call({"type": "state"})
                if done(state):
                    return state
            except (OSError, ValueError):  # the control server is not answering yet
                pass
            time.sleep(3)
        raise SystemExit("timed out waiting for the game")

    def tap(key, hold):
        call({"type": "input", "key": key, "action": "press"})
        time.sleep(hold)
        call({"type": "input", "key": key, "action": "release"})

    def actors():
        found, offset = {}, 0
        while True:
            page = call({"type": "scripts", "offset": offset, "limit": 64})
            found.update({actor["name"]: actor for actor in page["actors"]})
            if len(page["actors"]) < 64:
                found["total"], found["loop_frame"] = page["total"], page["globals"]["world_loop_frame"]
                return found
            offset += 64

    def bearing(me, other):
        dx, dz = (
            (other["spatial"]["effective"]["position"][axis] - me["spatial"]["effective"]["position"][axis]) / 256
            for axis in (0, 2)
        )
        return (-math.atan2(dx, dz) * 2048 / math.tau) % 2048, math.hypot(dx, dz)

    def follow_story(defeated):
        """Report the actor-count changes and the world change that follow the duel."""
        seen, scene = defeated["actors"], call({"type": "state"})["scene"]
        for _ in range(300):
            time.sleep(1)
            state = call({"type": "state"})
            if state["scene"] != scene:
                print(json.dumps({"t": round(time.time() - t0, 1), "event": "world change", "scene": state["scene"]}), flush=True)
                return
            try:
                total = actors()["total"]
            except SystemExit:  # the script world is torn down while the next world prepares
                print(json.dumps({"t": round(time.time() - t0, 1), "event": "world change", "scene": "loading"}), flush=True)
                return
            if total != seen:
                seen = total
                print(json.dumps({"t": round(time.time() - t0, 1), "event": "actors", "count": total}), flush=True)
        print("no world change within 300 s after the duel")

    wait(lambda state: state["state"] == "Menu")
    call({"type": "load_scene", "name": "START"})
    wait(lambda state: state["state"] == "InGame" and state["scene_ready"])
    time.sleep(10)  # the opening settles before the player accepts input
    call({"type": "cheat", "name": "drevil"})
    t0 = time.time()
    for step in range(args.limit):
        state = call({"type": "state"})
        if state["state"] != "InGame":
            sys.exit(f"left the world: {state['state']}")
        found = actors()
        me = found["CYRUS"]
        pirates = [found[name] for name in ("SPIRATE1", "SPIRATE2") if name in found]
        alive = [p for p in pirates if p["attributes"][3] > 0 and not (p["control"] or {}).get("killed")]
        # Same sample schema as original-duel.py: seconds, loop frame, player and pirate health.
        sample = {"t": round(time.time() - t0, 1), "mainloop_frame": found["loop_frame"], "player": me["attributes"][3],
                  "pirates": [(p["name"], p["attributes"][3]) for p in pirates], "actors": found["total"]}
        print(json.dumps(sample), flush=True)
        if not alive:
            print("both pirates defeated", json.dumps(sample), flush=True)
            follow_story(sample)
            return
        if not (me["control"] or {}).get("weapon_drawn"):
            tap("KeyS", 0.5)  # the quick sword key is ignored while the player is reacting to a hit
            continue
        target = min(alive, key=lambda p: bearing(me, p)[1])
        want, distance = bearing(me, target)
        current = me["spatial"]["effective"]["angles"][1]
        diff = (want - current + 1024) % 2048 - 1024
        if abs(diff) > 70:
            tap("ArrowLeft" if diff > 0 else "ArrowRight", 0.25)  # Left raises the yaw
        elif distance > 120:
            tap("ArrowUp", 0.6)
        else:
            tap("ControlLeft", 0.3)
    sys.exit("pirates not defeated within the step limit")


if __name__ == "__main__":
    main()
