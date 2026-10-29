#!/usr/bin/env python3
"""Watch the original game idle in one world: the selected player's health and state over time.

Sets `start_world` in the run copy's WORLD.INI (reset to 0, START, afterwards), starts the original in the
retro container as original-duel.py does (same requirements, state reader under
$CYRUS_MAGIC/tools), sends no input unless --hold names keys, and prints one JSON line per sample (wall seconds, the engine's
game seconds, engine frame, player health, hit inhibition, current and pending animation group, drawn-weapon byte, [sliding, falling, queued, living, motion byte, surface], position, actor count, nearby combat
actors as [id, health, group, death selector, distance]) for
--seconds. Compare with the same world's native idle run: a player that dies or loses health in
one and not the other is a behavioural difference.

    dev/scripts/original-idle.py --world 2 [--seconds 90]
"""
import argparse
import json
import os
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAGIC = Path(os.environ.get("CYRUS_MAGIC", ROOT / "magic"))


def sh(*cmd, check=True):
    return subprocess.run(cmd, capture_output=True, text=True, check=check)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--world", type=int, required=True, help="WORLD.INI index to start in")
    parser.add_argument("--seconds", type=int, default=90)
    parser.add_argument("--hold", action="append", default=[], metavar="KEY:START:END",
                        help="hold an xdotool key from START to END seconds after the world is live (repeatable), e.g. Up:3:9")
    parser.add_argument("--all", action="store_true", help="also list every actor as [id, group, x, z, health, death selector]")
    parser.add_argument("--interval", type=float, default=2.0, help="seconds between samples (each read takes about 0.3)")
    args = parser.parse_args()
    run = MAGIC / "retro-run"
    ini = run / "Redguard/WORLD.INI"
    original = re.sub(r"(?m)^start_world=\d+", "start_world=0", ini.read_text(errors="surrogateescape"))  # the install's START
    name = "retro-idle"
    reader = MAGIC / "tools/original-state.py"
    if not reader.exists():
        sys.exit(f"{reader} is missing; restore it from redguard-decomp research/supplements/2026-10-23-original-duel-compare")
    try:
        ini.write_text(re.sub(r"(?m)^start_world=\d+", f"start_world={args.world}", original), errors="surrogateescape")
        sh("docker", "rm", "-f", name, check=False)
        sh("docker", "run", "--rm", "--entrypoint", "find", "-v", f"{run}:/game", "cyrus-retro:dev",
           "/game/Redguard/maps", "-iname", "*.tsg", "-delete", check=False)
        sh("docker", "run", "-d", "--rm", "--name", name, "--cap-add", "SYS_PTRACE", "-v", f"{run}:/game",
           "-v", f"{MAGIC}/gog:/gog:ro", "-v", f"{MAGIC}/gog/extracted/app/game.gog:/game/game.gog:ro", "cyrus-retro:dev")
        sh("docker", "cp", str(reader), f"{name}:/tmp/original-state.py")

        def state():
            out = sh("docker", "exec", name, "python3", "/tmp/original-state.py", check=False)
            return json.loads(out.stdout) if out.returncode == 0 and out.stdout else None

        started = time.time()
        while True:
            if time.time() - started > 300:
                sys.exit("the original did not reach a live world")
            s = state()
            me = next((a for a in s["actors"] if a["selected"]), None) if s else None
            if me and s["counters"]["mainloop_frame"] > 0:
                break
            time.sleep(2)
        t0 = time.time()
        game = {"s": 0.0, "frame": s["counters"]["mainloop_frame"]}  # frames over the engine's own fps
        holds = [(key, float(a), float(b)) for key, a, b in (h.split(":") for h in args.hold)]
        down = set()
        while time.time() - t0 < args.seconds:
            now = time.time() - t0
            for key, begin, end in holds:
                if begin <= now < end and key not in down:
                    sh("docker", "exec", "-e", "DISPLAY=:1", name, "xdotool", "keydown", key, check=False)
                    down.add(key)
                elif now >= end and key in down:
                    sh("docker", "exec", "-e", "DISPLAY=:1", name, "xdotool", "keyup", key, check=False)
                    down.discard(key)
            s = state()
            if s:
                me = next((a for a in s["actors"] if a["selected"]), None)
                if me:
                    frames = s["counters"]["mainloop_frame"] - game["frame"]
                    game["frame"] = s["counters"]["mainloop_frame"]
                    if frames > 0 and s["counters"]["fps"] > 0:
                        game["s"] += frames / s["counters"]["fps"]
                    print(json.dumps({"t": round(time.time() - t0, 1), "game_s": round(game["s"], 2), "frame": s["counters"]["mainloop_frame"], "player": me["health"],
                                      "hit_inhibited": s["hit_inhibited"], "group": me["group"], "pending": me.get("pending"), "drawn": me.get("drawn"),
                                      "motion": [me.get(k) for k in ("sliding", "falling", "queued", "living", "motion", "surface")], "position": me["position"], "yaw": me.get("yaw"),
                                      "actors": len(s["actors"]), "attrs": me.get("attrs"),
                                      "near": [[a["id"], a["health"], a["group"], a["death_selector"],
                                                round(((a["position"][0] - me["position"][0]) ** 2 + (a["position"][2] - me["position"][2]) ** 2) ** 0.5)]
                                               for a in s["actors"] if a["combat"] and not a["selected"]
                                               and ((a["position"][0] - me["position"][0]) ** 2 + (a["position"][2] - me["position"][2]) ** 2) ** 0.5 < 2500],
                                      **({"all": [[a["id"], a["group"], a["position"][0], a["position"][2], a["health"], a["death_selector"]]
                                                  for a in s["actors"] if not a["selected"]]} if args.all else {})}), flush=True)
            time.sleep(args.interval)
    finally:
        ini.write_text(original, errors="surrogateescape")
        sh("docker", "rm", "-f", name, check=False)


if __name__ == "__main__":
    main()
