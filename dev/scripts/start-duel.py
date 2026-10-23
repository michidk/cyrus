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
            except OSError:  # the control server is not listening yet
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
                return found
            offset += 64

    def bearing(me, other):
        dx, dz = (
            (other["spatial"]["effective"]["position"][axis] - me["spatial"]["effective"]["position"][axis]) / 256
            for axis in (0, 2)
        )
        return (-math.atan2(dx, dz) * 2048 / math.tau) % 2048, math.hypot(dx, dz)

    wait(lambda state: state["state"] == "Menu")
    call({"type": "load_scene", "name": "START"})
    wait(lambda state: state["state"] == "InGame" and state["scene_ready"])
    time.sleep(10)  # the opening settles before the player accepts input
    call({"type": "cheat", "name": "drevil"})
    tap("KeyS", 1.0)  # quick sword
    left_is_positive = None
    for step in range(args.limit):
        state = call({"type": "state"})
        if state["state"] != "InGame":
            sys.exit(f"left the world: {state['state']}")
        found = actors()
        me = found["CYRUS"]
        pirates = [found[name] for name in ("SPIRATE1", "SPIRATE2") if name in found]
        alive = [p for p in pirates if p["attributes"][3] > 0 and not (p["control"] or {}).get("killed")]
        if step % 5 == 0:
            print(step, state["frame"], [(p["name"], p["attributes"][3]) for p in pirates], "player", me["attributes"][3], flush=True)
        if not alive:
            print("both pirates defeated at frame", state["frame"])
            return
        target = min(alive, key=lambda p: bearing(me, p)[1])
        want, distance = bearing(me, target)
        current = me["spatial"]["effective"]["angles"][1]
        diff = (want - current + 1024) % 2048 - 1024
        if abs(diff) > 70:
            key = "ArrowLeft" if (diff > 0) == (left_is_positive is not False) else "ArrowRight"
            tap(key, 0.25)
            if left_is_positive is None:  # calibrate the turn direction from the first turn
                after = actors()["CYRUS"]["spatial"]["effective"]["angles"][1]
                left_is_positive = ((after - current + 1024) % 2048 - 1024) * diff > 0
        elif distance > 120:
            tap("ArrowUp", 0.6)
        else:
            tap("ControlLeft", 0.3)
    sys.exit("pirates not defeated within the step limit")


if __name__ == "__main__":
    main()
