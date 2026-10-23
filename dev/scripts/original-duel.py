#!/usr/bin/env python3
"""Play the START pirate duel in the original game with the same bot policy as start-duel.py.

Starts the original under DOSBox-X in the retro container (needs retro-run, gog and the extracted
GOG app under $CYRUS_MAGIC, default ./magic, as run-retro.sh does). The container gets
--cap-add SYS_PTRACE so the state reader can read the emulator's guest RAM; it must not run in a
tracked file because it holds executable addresses, so it lives in the gitignored
$CYRUS_MAGIC/tools/original-state.py and is archived in the private redguard-decomp repository
(research/supplements/2026-10-23-original-duel-compare). The bot turns toward, approaches and
attacks the nearest living pirate with installed keys sent through xdotool, reading positions,
yaw and health from the original's own memory. Prints one JSON line per sample (wall seconds,
the original's game seconds, engine frame counters, player and pirate health), then the story
events up to the world change; exits 0 when both pirates are defeated and the story moves on.
Use --attach NAME to drive a container that is already running START.

    dev/scripts/original-duel.py [--attach NAME] [--limit 400] [--log duel.jsonl]
"""
import argparse
import json
import math
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAGIC = Path(os.environ.get("CYRUS_MAGIC", ROOT / "magic"))  # holds retro-run and gog


def sh(*cmd, check=True):
    return subprocess.run(cmd, capture_output=True, text=True, check=check)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--attach", help="container already running the original in START")
    parser.add_argument("--limit", type=int, default=400, help="decision steps before giving up")
    parser.add_argument("--log", help="write every sample as JSON lines")
    args = parser.parse_args()
    name = args.attach or "retro-duel"
    if not args.attach:
        run = MAGIC / "retro-run"
        sh("docker", "rm", "-f", name, check=False)
        # Leaving a world saves MAPS\\<world>.TSG; the game hangs if that write fails, so mount a
        # fresh writable copy of the (otherwise read-only) maps directory over the linked one.
        maps = MAGIC / "retro-maps-run"
        # The container writes as root, so remove the previous copy through it.
        sh("docker", "run", "--rm", "--entrypoint", "rm", "-v", f"{MAGIC}:/m", "cyrus-retro:dev", "-rf", "/m/retro-maps-run")
        shutil.copytree(MAGIC / "gog/extracted/app/Redguard/maps", maps)
        sh("docker", "run", "-d", "--rm", "--name", name, "--cap-add", "SYS_PTRACE",
           "-v", f"{run}:/game", "-v", f"{MAGIC}/gog:/gog:ro",
           "-v", f"{MAGIC}/gog/extracted/app/game.gog:/game/game.gog:ro", "cyrus-retro:dev")
    reader = MAGIC / "tools/original-state.py"
    if not reader.exists():
        sys.exit(f"{reader} is missing; restore it from redguard-decomp research/supplements/2026-10-23-original-duel-compare")
    sh("docker", "cp", str(reader), f"{name}:/tmp/original-state.py")
    log = open(args.log, "w") if args.log else None
    started = time.time()

    def state():
        out = sh("docker", "exec", name, "python3", "/tmp/original-state.py", check=False)
        return json.loads(out.stdout) if out.returncode == 0 and out.stdout else None

    def key(action, name_):
        sh("docker", "exec", "-e", "DISPLAY=:1", name, "xdotool", action, name_)

    def tap(name_, hold):
        key("keydown", name_)
        time.sleep(hold)
        key("keyup", name_)

    # Wait for START to be live with a selected, living player.
    while True:
        if time.time() - started > 300:
            sys.exit("the original did not reach a live START")
        s = state()
        me = next((a for a in s["actors"] if a["selected"]), None) if s else None
        if me and me["health"] and s["counters"]["mainloop_frame"] > 0:
            break
        time.sleep(2)
    t0 = time.time()
    # Game time: the engine scales every frame by its measured frames per BIOS second, so frames
    # advanced divided by that rate is its own clock (wall time differs while the emulator lags).
    game = {"s": 0.0, "frame": s["counters"]["mainloop_frame"]}

    def game_seconds(counters):
        frames = counters["mainloop_frame"] - game["frame"]
        game["frame"] = counters["mainloop_frame"]
        if frames > 0 and counters["fps"] > 0:
            game["s"] += frames / counters["fps"]
        return round(game["s"], 1)

    if not me["drawn"]:
        tap("s", 0.4)  # quick sword

    def bearing(a, b):
        dx, dz = b["position"][0] - a["position"][0], b["position"][2] - a["position"][2]
        return (-math.atan2(dx, dz) * 2048 / math.tau) % 2048, math.hypot(dx, dz)

    pirates = None
    defeated_at = None
    for step in range(args.limit):
        s = state()
        if not s:
            continue
        me = next(a for a in s["actors"] if a["selected"])
        if pirates is None:  # the living combat actors at the start, as in the native script
            pirates = [a["pointer"] for a in s["actors"] if a["combat"] and not a["selected"] and a["health"] and a["health"] < 100]
        found = [a for a in s["actors"] if a["pointer"] in pirates]
        alive = [a for a in found if a["health"] > 0 and a["death_selector"] != 8]
        sample = {"t": round(time.time() - t0, 1), "game_s": game_seconds(s["counters"]), **s["counters"], "player": me["health"],
                  "pirates": [(a["id"], a["health"]) for a in found], "actors": len(s["actors"])}
        print(json.dumps(sample), flush=True)
        if log:
            log.write(json.dumps({**sample, "me": me, "found": found}) + "\n")
        if me["health"] == 0:
            print("player defeated"); return 1
        if not alive:
            defeated_at = sample
            break
        target = min(alive, key=lambda a: bearing(me, a)[1])
        want, distance = bearing(me, target)
        diff = (want - me["yaw"] + 1024) % 2048 - 1024
        if abs(diff) > 70:
            tap("Left" if diff > 0 else "Right", 0.25)  # Left raises the yaw
        elif distance > 120:
            tap("Up", 0.6)
        else:
            tap("Control_L", 0.3)
    if not defeated_at:
        print("pirates not defeated within the step limit"); return 1
    print("both pirates defeated", json.dumps(defeated_at))
    # Follow the story: the world changes when the engine stops advancing frames to load it.
    last, stalled_since, seen_actors = None, None, defeated_at["actors"]
    for _ in range(300):
        time.sleep(1)
        s = state()
        if not s:
            continue
        frame = s["counters"]["mainloop_frame"]
        if len(s["actors"]) != seen_actors:
            seen_actors = len(s["actors"])
            print(json.dumps({"t": round(time.time() - t0, 1), "game_s": game_seconds(s["counters"]), "event": "actors", "count": seen_actors, "frame": frame}), flush=True)
        if frame == last:
            stalled_since = stalled_since or time.time()
            if time.time() - stalled_since > 4:
                print(json.dumps({"t": round(stalled_since - t0, 1), "game_s": game_seconds(s["counters"]), "event": "world change", "frame": frame}))
                return 0
        else:
            last, stalled_since = frame, None
    print("no world change within 240 s after the duel")
    return 0


if __name__ == "__main__":
    sys.exit(main())
