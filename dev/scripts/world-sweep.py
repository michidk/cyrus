#!/usr/bin/env python3
"""Soak installed worlds through the control port and report failed script actors.

For each world the sweep loads it with `load_scene`, waits for the scene to be ready, lets the
ordinary updates run for `--seconds`, and prints every actor whose script reported a failure
(once per distinct message). An optional `--flag WORLD:NAME=VALUE` first sets a global flag
through the console, which starts the story scene that polls it (for example
`CATACOMB:scene2=1`, `JAILINT:scene5=1`, `MGUILD:gremlin_scene=1`, `START:deadpirates=2`).
Exits 0 when no world failed to load, no script world failed or stalled and no actor failed, 1 otherwise. Needs a running native
cyrus with CYRUS_CONTROL_ADDR set (a real-time audio sink, see docs/SETUP.md, lets voiced lines finish at their own pace), for example

    dev/scripts/world-sweep.py --port 7117 --seconds 60 START ISLAND DRINT
    dev/scripts/world-sweep.py --port 7117 --seconds 240 --flag CATACOMB:scene2=1 CATACOMB
"""
import argparse
import json
import socket
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("worlds", nargs="+", help="scene names as for load_scene, e.g. START")
    parser.add_argument("--port", type=int, default=7117)
    parser.add_argument("--seconds", type=int, default=60, help="soak time per world")
    parser.add_argument("--ready-timeout", type=int, default=300)
    parser.add_argument("--flag", action="append", default=[], metavar="WORLD:NAME=VALUE")
    args = parser.parse_args()
    flags = {}
    for entry in args.flag:
        world, _, assignment = entry.partition(":")
        flags.setdefault(world, []).append(assignment.replace("=", ","))

    def call(cmd):
        with socket.create_connection(("127.0.0.1", args.port), timeout=300) as conn:
            conn.sendall((json.dumps({"id": 1, "cmd": cmd}) + "\n").encode())
            buf = b""
            while not buf.endswith(b"\n"):
                chunk = conn.recv(1 << 24)
                if not chunk:
                    raise ConnectionError("control port closed")
                buf += chunk
        reply = json.loads(buf)
        if not reply.get("ok", True):
            raise RuntimeError(reply.get("error", "control request failed"))
        return reply.get("result", reply)

    def wait_ready(world):
        deadline = time.time() + args.ready_timeout
        while time.time() < deadline:
            try:
                state = call({"type": "state"})
                if (
                    state["state"] == "InGame"
                    and state.get("scene_ready")
                    and state["scene"] == world
                ):
                    return True
            except (OSError, RuntimeError, KeyError):
                pass
            time.sleep(3)
        return False

    def script_identity(page):
        return tuple(page.get(key) for key in ("world", "generation", "content_id"))

    def script_loop_sample():
        try:
            page = call({"type": "scripts", "offset": 0, "limit": 1})
        except (OSError, RuntimeError, KeyError):
            return (None, None, None), None
        globals_ = page.get("globals") or {}
        return script_identity(page), globals_.get("world_loop_frame")

    def failures(identity):
        found, offset = {}, 0
        while True:
            page = call({"type": "scripts", "offset": offset, "limit": 64})
            if script_identity(page) != identity:
                found[("script inspection", "world identity changed during actor pagination")] = True
                return found
            if not page.get("actors"):
                return found
            for actor in page["actors"]:
                if actor.get("failure"):
                    found[(actor["name"], actor["failure"])] = True
            offset += len(page["actors"])

    bad = 0
    for world in args.worlds:
        call({"type": "load_scene", "name": world})
        time.sleep(8)
        if not wait_ready(world):
            print(f"{world}: not ready after {args.ready_timeout} s")
            bad += 1
            continue
        for assignment in flags.get(world, []):
            call({"type": "console", "open": True})
            call({"type": "console", "line": f"setflag {assignment}"})
            call({"type": "console", "open": False})
        game0 = (call({"type": "state"}).get("clock") or {}).get("game_seconds", 0)
        identity0, loop0 = script_loop_sample()
        seen, end = {}, time.time() + args.seconds
        while True:
            seen.update(failures(identity0))
            if time.time() >= end:
                break
            time.sleep(min(15, max(1, end - time.time())))
        for (name, message) in seen:
            print(f"{world}: {name}: {message[:300]}")
        # A failed script world stops ticking without any actor reporting it, so check the world itself.
        final = call({"type": "state"})
        world_error = (final.get("scripts") or {}).get("error")
        ticked = (final.get("clock") or {}).get("game_seconds", 0) - game0
        identity1, loop1 = script_loop_sample()
        identity_changed = identity0 != identity1 or None in identity0
        loop_stalled = loop0 is None or loop1 is None or loop1 == loop0
        clock_stalled = ticked < min(args.seconds, 30) / 10
        stalled = clock_stalled or loop_stalled or identity_changed
        if identity_changed:
            print(f"{world}: script world identity changed or unavailable during soak")
        if loop_stalled:
            print(f"{world}: script world_loop_frame did not advance ({loop0})")
        if world_error:
            print(f"{world}: script world failed: {world_error[:300]}")
        if clock_stalled:
            print(f"{world}: game clock advanced only {ticked:.1f} s in {args.seconds} s")
        print(f"{world}: {len(seen)} failing actors")
        bad += bool(seen) or bool(world_error) or stalled
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
