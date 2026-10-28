#!/usr/bin/env python3
"""Exercise combat actors' lost-sight routing in installed worlds over the control port.

For each world: load it, place the selected player a short distance from the nearest living
combat actor (so the actor engages), then hold the player far away (so the actor loses sight
and, after the combat node timer, routes toward the player). Reports failed actors, a world-level
script error, and how many actors reached lost-sight timing and routed pursuit.

Needs a native build with the control port (see docs/SETUP.md) and an install. Run one world per
process when comparing runs: `load_scene` keeps the selected player's health across worlds.
"""
import argparse
import json
import math
import socket
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("worlds", nargs="+")
    parser.add_argument("--port", type=int, default=7117)
    parser.add_argument("--near", type=int, default=40, help="map units from the actor at first")
    parser.add_argument("--far", type=int, default=700, help="map units away afterwards")
    parser.add_argument("--far-toward-spawn", action="store_true", help="put the far position --far units from the actor along the line to the player's spawn instead of east")
    parser.add_argument("--dwells", type=int, default=13, help="control polls (the first is near)")
    parser.add_argument("--dwell-seconds", type=int, default=20)
    args = parser.parse_args()

    def call(cmd, timeout=120):
        with socket.create_connection(("127.0.0.1", args.port), timeout=timeout) as conn:
            conn.sendall((json.dumps({"id": 1, "cmd": cmd}) + "\n").encode())
            buf = b""
            while not buf.endswith(b"\n"):
                chunk = conn.recv(1 << 24)
                if not chunk:
                    break
                buf += chunk
        reply = json.loads(buf)
        return reply.get("result", reply)

    def actors():
        out, offset = [], 0
        while True:
            page = call({"type": "scripts", "offset": offset, "limit": 64})
            out += page["actors"]
            if len(page["actors"]) < 64:
                return out
            offset += 64

    def ready(world, tries=150):
        for _ in range(tries):
            try:
                state = call({"type": "state"})
                if state.get("state") == "InGame" and state.get("scene_ready") and state.get("scene") == world:
                    return True
            except (OSError, RuntimeError, KeyError):
                pass
            time.sleep(3)
        return False

    def combat_state(actor):
        return (actor.get("control") or {}).get("combat")

    bad = 0
    for world in args.worlds:
        call({"type": "load_scene", "name": world})
        time.sleep(8)
        if not ready(world):
            print(f"{world}: not ready")
            bad += 1
            continue
        time.sleep(15)
        listed = actors()
        player = next((a for a in listed if a.get("source") == "player" and a.get("spatial")), None)
        if player is None:
            print(f"{world}: no script player")
            bad += 1
            continue
        origin = player["spatial"]["effective"]["position"]
        hostile = [
            a
            for a in listed
            if a.get("spatial") and combat_state(a) and a.get("source") != "player" and a["attributes"][3] > 0
        ]
        if not hostile:
            print(f"{world}: no combat actors")
            continue
        target = min(hostile, key=lambda a: math.dist(a["spatial"]["effective"]["position"], origin))
        spot = target["spatial"]["effective"]["position"]
        print(f"{world}: target {target['name']}")
        failures, lost_max, routing_max, world_error = {}, 0, 0, None
        for dwell in range(args.dwells):
            offset = args.far if dwell else args.near
            if dwell and args.far_toward_spawn:
                dx, dz = origin[0] - spot[0], origin[2] - spot[2]
                norm = math.hypot(dx, dz) or 1.0
                x = int(spot[0] // 256 + dx / norm * args.far)
                y = spot[1] // 256
                z = int(spot[2] // 256 + dz / norm * args.far)
            else:
                x, y, z = spot[0] // 256 + offset, spot[1] // 256, spot[2] // 256
            call({"type": "place_player", "x": x, "y": y, "z": z, "heading": 0})
            time.sleep(args.dwell_seconds)
            listed = actors()
            for actor in listed:
                if actor.get("failure"):
                    failures[(actor["name"], actor["failure"][:240])] = True
            others = [a for a in listed if combat_state(a) and a.get("source") != "player"]
            lost_max = max(lost_max, sum(1 for a in others if combat_state(a).get("lost_sight_ticks", 0) > 0))
            routing_max = max(routing_max, sum(1 for a in others if combat_state(a).get("lost_sight_routing", 0) > 0))
            world_error = (call({"type": "state"}).get("scripts") or {}).get("error")
            # The target's horizontal distance to the player in map units, for comparison runs.
            here = next((a for a in listed if a.get("source") == "player" and a.get("spatial")), None)
            there = next((a for a in listed if a.get("id") == target.get("id") and a.get("spatial")), None)
            if here and there:
                a, b = here["spatial"]["effective"]["position"], there["spatial"]["effective"]["position"]
                print(
                    f"{world}: dwell {dwell} target {target['name']} distance {math.hypot(a[0] - b[0], a[2] - b[2]) / 256:.0f}"
                    f" group {(there.get('animation') or {}).get('current_group')}"
                    f" routing {combat_state(there).get('lost_sight_routing', 0)}"
                )
        for name, message in failures:
            print(f"{world}: {name}: {message}")
        if world_error:
            print(f"{world}: script world failed: {world_error[:300]}")
        print(f"{world}: {len(failures)} failing actors, max lost-sight {lost_max}, max routing {routing_max}")
        bad += bool(failures) or bool(world_error)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
