#!/usr/bin/env python3
"""Check that New Game after a death starts from fresh state, through the control port.

Needs a running native cyrus (launched with --skip-startup and no --scene, an install whose parent
holds the CD image so the introduction movie plays) with CYRUS_CONTROL_ADDR set. Loads START from
the menu, snapshots the script globals and HUD after the opening settles, waits for the idle player
to die in the START fight, chooses NEW in the death book (skipping INTRO.SMK with the Enter key),
snapshots again and prints the differences. Timing-driven values (frame counters, health while the
fight runs, the idle counter) are listed but expected to differ; inventory, flags, compass and
power-ups must not.

    dev/scripts/new-game-compare.py [--port 7117] [--cycles 3]
"""
import argparse
import json
import socket
import sys
import time

TIMING = {"player_idle_counter", "sound_maintenance", "world_loop_frame", "selected_health", "generation", "highlighted_choice"}


def strip_generation(source):
    """The retained native source without its actor generation (`{"Actor": {"index", "generation"}}`)."""
    actor = source.get("Actor") if isinstance(source, dict) else None
    if isinstance(actor, dict):
        return {"Actor": {"index": actor.get("index")}}
    return source


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--port", type=int, default=7117)
    parser.add_argument("--cycles", type=int, default=1, help="death and New Game repetitions, each compared with the first load")
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
            sys.exit(f"control {cmd['type']}: {reply.get('error')}")
        return reply["result"]

    def wait(done, limit=300):
        end = time.time() + limit
        while time.time() < end:
            try:
                state = call({"type": "state"})
                if done(state):
                    return state
            except (OSError, ValueError, SystemExit):
                pass
            time.sleep(2)
        sys.exit("timed out waiting for the game")

    def tap(key, hold=0.3):
        call({"type": "input", "key": key, "action": "press"})
        time.sleep(hold)
        call({"type": "input", "key": key, "action": "release"})
        time.sleep(0.4)

    def snapshot():
        state, offset = call({"type": "state"}), 0
        while True:
            page = call({"type": "scripts", "offset": offset, "limit": 64})
            if len(page["actors"]) < 64:
                break
            offset += 64
        globals_ = {k: v for k, v in page["globals"].items() if k not in ("combat_messages", "dialogue", "player_contacts", "group_sync", "printed")}
        # The book-owned screens and the sound mixer are part of the session a New Game resets.
        extra = {key: state.get(key) for key in ("logbook", "inventory_screen", "island_map", "cheats", "sounds", "player_input")}
        return {"hud": state["hud"], "script_hud": state["script_hud"], "globals": globals_, "extra": extra, "actors": page["total"]}

    def choose_new():
        for _ in range(12):
            menu = call({"type": "state"})["menu"]
            if menu["selected"] is not None and menu["entries"][menu["selected"]]["action"] == 10:
                tap("Enter")
                break
            tap("ArrowDown")
        else:
            sys.exit("NEW is not selectable in the death book")
        for _ in range(60):  # INTRO.SMK plays first; the key skips to the installed skip points
            if call({"type": "state"})["state"] == "InGame":
                break
            tap("Enter", 0.2)
            time.sleep(2)
        wait(lambda s: s["state"] == "InGame" and s["scene_ready"])

    wait(lambda s: s["state"] == "Menu" and (s["menu"] or {}).get("ready"))
    call({"type": "load_scene", "name": "START"})
    wait(lambda s: s["state"] == "InGame" and s["scene_ready"])
    time.sleep(15)
    first = snapshot()
    bad = False
    for cycle in range(1, args.cycles + 1):
        tap("KeyS", 1.0)
        wait(lambda s: s["state"] == "Menu" and (s["menu"] or {}).get("context") == "death", 600)
        wait(lambda s: (s["menu"] or {}).get("ready"))
        choose_new()
        time.sleep(15)
        second = snapshot()
        print(f"cycle {cycle}")
        for section in ("hud", "script_hud", "globals", "extra"):
            for key in sorted(set(first[section] or {}) | set(second[section] or {})):
                a, b = (first[section] or {}).get(key), (second[section] or {}).get(key)
                if key == "native_source":
                    # The retained native-command source is an actor identity; a world re-entry gives the
                    # same actor a new generation, so only the index is compared.
                    a, b = (strip_generation(a), strip_generation(b))
                if a != b:
                    expected = section == "hud" or key in TIMING or key in ("sounds", "player_input", "logbook_entries")  # a scene-started baseline carries the installed default journal; New Game starts it empty
                    bad |= not expected
                    print(f"{'timing' if expected else 'DIFFERS'} {section}.{key}: {json.dumps(a)[:120]} -> {json.dumps(b)[:120]}")
        if first["actors"] != second["actors"]:
            bad = True
            print(f"DIFFERS actors: {first['actors']} -> {second['actors']}")
    print("fresh state after New Game" if not bad else "stale state after New Game")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
