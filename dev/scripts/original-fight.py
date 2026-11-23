#!/usr/bin/env python3
"""Measure the original game's Richton duel on EXTPALAC, driven by the original's own frame counter.

Starts the original under DOSBox-X in the retro container (image `cyrus-retro:dev`, built from
dev/docker/retro) with a run copy of the install under $CYRUS_MAGIC/retro-run-<name>, never the shared
read-only game links. One streaming sampler inside the container reads every actor's group,
health, pending group and drawn-sword byte several times a second, so there is no per-iteration
docker exec; keys go through xdotool. Every wait counts the original's `mainloop_frame`, not wall
time: on a loaded host the original runs at a few frames per second and fixed wall-clock waits
expire before a draw or a swing finishes.

The state reader and the memory-placement helper hold executable addresses, so they stay in the
gitignored $CYRUS_MAGIC/tools/original-state.py and $CYRUS_MAGIC/helpers/orig_write_in.py (never
commit them). Output is one JSON object per sampled frame in --out, then a summary on stdout.

    CYRUS_MAGIC=~/workspaces/cyrus/magic python3 dev/scripts/original-fight.py \\
        --name soup4 --out run.jsonl --max-frames 1500 --cheat drevil

The summary reports taps, hits, damage per hit, each tap's outcome (hit, parried or countered by the
opponent's reaction group, or missed) split by whether the opponent was idle or busy, and the
frames and game seconds to the end of the run.
"""
import argparse
import fcntl
import json
import math
import os
import re
import shutil
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from diag_output import DurableLog, verify
from original_console import Console, frame_hold

ROOT = Path(__file__).resolve().parents[2]
MAGIC = Path(os.environ.get("CYRUS_MAGIC", ROOT / "magic"))
OPPONENT_ID = 55  # RICHTONP
ARMED_IDLE_GROUP = 8
NOT_ENGAGED_GROUP = 0
# The installed EXTPALAC RICHTONP script begins combat only while HaveItem(79) is 1 and SOUP
# PlayerDistance() < 131072 (preservation SOUP.md "PlayerDistance and PlayerFacing"): the sum of the
# absolute X, Y and Z differences in Q8 runtime units, so height counts, not just the courtyard walk.
ENGAGE_RANGE_Q8 = 131072
STATE_EVERY_FRAMES = 20  # one state row per this many original frames during the fight
HIT_REACTION_GROUP = 117  # a nonplayer hit receiver's reaction (preservation SOUP.md)
SAMPLER = """\
import contextlib, importlib.util, io, json, sys, time
spec = importlib.util.spec_from_file_location("st", "/tmp/original-state.py")
st = importlib.util.module_from_spec(spec)
spec.loader.exec_module(st)
interval = float(sys.argv[1])
while True:
    out = io.StringIO()
    try:
        with contextlib.redirect_stdout(out):
            st.main()
        print(out.getvalue().strip().replace("\\n", " "), flush=True)
    except (Exception, SystemExit) as error:  # not started yet or the world is changing: keep sampling
        print(json.dumps({"error": str(error)}), flush=True)
    time.sleep(interval)
"""


def view(state):
    """The fields the policy needs from one reader sample, or None without a selected player.
    The opponent fields are None until he exists (he joins the actor list late in the approach)."""
    if not state or "actors" not in state:
        return None
    player = next((a for a in state["actors"] if a["selected"]), None)
    if player is None:
        return None
    opponent = next((a for a in state["actors"] if a["id"] == OPPONENT_ID and not a["selected"]), None)
    return {
        "frame": state["counters"]["mainloop_frame"],
        "fps": state["counters"]["fps"],
        "pg": player["group"], "ph": player["health"], "pdrawn": player["drawn"],
        "rg": opponent["group"] if opponent else None,
        "rpending": opponent["pending"] if opponent else None,
        "rh": opponent["health"] if opponent else None,
        # Q8 positions and yaw, for distance and step-back analysis against the native snapshots.
        "ppos": player["position_q8"], "pyaw": player["yaw"],
        "rpos": opponent["position_q8"] if opponent else None,
        "ryaw": opponent["yaw"] if opponent else None,
    }


def player_distance_q8(sample):
    """SOUP PlayerDistance(): |dx| + |dy| + |dz| in Q8 units between the fighters, or None."""
    if sample["rpos"] is None:
        return None
    return sum(abs(a - b) for a, b in zip(sample["rpos"], sample["ppos"]))


def horizontal_distance(sample):
    """Horizontal Q8 distance between the fighters (map X and Z), or None while he is absent."""
    if sample["rpos"] is None:
        return None
    return round(math.hypot(sample["rpos"][0] - sample["ppos"][0], sample["rpos"][2] - sample["ppos"][2]))


def opponent_busy(sample):
    """The opponent is attacking, defending or reacting: any group other than idle, not engaged
    or absent."""
    return sample["rg"] not in (NOT_ENGAGED_GROUP, ARMED_IDLE_GROUP, None)


def ready_to_tap(sample, gate):
    """A tap goes only from the player's armed idle animation (group 8 is reached only with the
    sword drawn; the reader's drawn byte stays 0 there, so the group is the signal); the `busy`
    gate additionally waits for the opponent to be mid-action (an idle opponent counters)."""
    if sample["pg"] != ARMED_IDLE_GROUP:
        return False
    return opponent_busy(sample) if gate == "busy" else True


def classify_tap(before, samples):
    """Outcome of one tap from the samples that followed it: the opponent's health drop is a hit
    (with its damage), his reaction group 117 without a drop is a hit absorbed elsewhere, and any
    other change in his group is a parry/counter response; none at all is a miss."""
    drop = before["rh"] - min((s["rh"] for s in samples), default=before["rh"])
    if drop > 0:
        return {"outcome": "hit", "damage": drop}
    groups = [s["rg"] for s in samples]
    if HIT_REACTION_GROUP in groups:
        return {"outcome": "reaction", "damage": 0}
    if any(g != before["rg"] for g in groups):
        return {"outcome": "response", "damage": 0, "groups": sorted(set(groups) - {before["rg"]})}
    return {"outcome": "miss", "damage": 0}


def summarize(rows):
    """Aggregate a run: per-tap outcomes split by opponent idle/busy, hits and damage per hit."""
    taps = [r for r in rows if r.get("ev") == "tap"]
    result = {"taps": len(taps), "hits": 0, "damage": [], "by_state": {}}
    for tap in taps:
        state = "busy" if opponent_busy(tap["before"]) else "idle"
        bucket = result["by_state"].setdefault(state, {"taps": 0, "hit": 0, "reaction": 0, "response": 0, "miss": 0})
        bucket["taps"] += 1
        bucket[tap["outcome"]] += 1
        if tap["outcome"] == "hit":
            result["hits"] += 1
            result["damage"].append(tap["damage"])
    ends = [r for r in rows if r.get("ev") == "end"]
    if ends:
        result["end"] = {k: ends[-1].get(k) for k in ("rh", "ph", "frames", "reason")}
    return result


class Sampler:
    """Streams reader samples from one long-lived docker exec and keeps the latest."""

    def __init__(self, name, interval):
        self.process = subprocess.Popen(
            ["docker", "exec", "-i", name, "python3", "-u", "-c", SAMPLER, str(interval)],
            stdout=subprocess.PIPE, text=True, bufsize=1)
        self.latest = None
        self.lock = threading.Lock()
        self.thread = threading.Thread(target=self.pump, daemon=True)
        self.thread.start()

    def pump(self):
        for line in self.process.stdout:
            try:
                state = json.loads(line)
            except ValueError:
                continue
            with self.lock:
                self.latest = state

    def get(self):
        with self.lock:
            return view(self.latest), self.latest

    def stop(self):
        self.process.kill()


def wait_frames(sampler, frames, timeout, until=None):
    """Block on the original's frame counter, never on wall time alone.

    Without `until`, return once the counter has advanced `frames`. With `until`, return the first
    sample for which it holds; a positive `frames` is then a budget and raises TimeoutError once
    that many frames pass without it holding. `timeout` is only a wall-clock safety limit."""
    deadline = time.monotonic() + timeout
    start = None
    while time.monotonic() < deadline:
        sample, raw = sampler.get()
        counter = (raw or {}).get("counters", {}).get("mainloop_frame") if raw else None
        if counter is not None:
            start = counter if start is None else start
            if until:
                if sample and until(sample):
                    return sample
                if frames and counter - start >= frames:
                    raise TimeoutError(f"condition not met within {frames} frames")
            elif counter - start >= frames:
                return sample
        time.sleep(0.05)
    raise TimeoutError(f"no {frames} frames within {timeout} s (counter start {start})")


def walk_step(sample, tolerance=100):
    """One held-key decision for the walk-in: turn toward the opponent, else walk forward.
    Returns (key, hold_seconds) or None once he is inside striking range and faced.
    Original turning is about 416 heading units per second and forward walking about 200 map units
    per second (measured earlier); holds are wall time because a held key is what registers at a few
    frames per second."""
    if sample["rpos"] is None:
        return None
    dx, dz = sample["rpos"][0] - sample["ppos"][0], sample["rpos"][2] - sample["ppos"][2]
    bearing = round(math.degrees(math.atan2(dx, dz)) * 2048 / 360) % 2048
    # The original's yaw is -atan2(dx, dz) in 2048 units (Right decreases yaw).
    want = (-bearing) % 2048
    error = (want - sample["pyaw"] + 1024) % 2048 - 1024
    if abs(error) > tolerance:
        return ("Left" if error > 0 else "Right", min(1.5, max(0.2, abs(error) / 416)))
    if horizontal_distance(sample) > 12000:
        return ("Up", 1.0)
    return None


def walk_in(sampler, key, frames):
    """Walk to the opponent with held keys until he leaves group 0 or the frame budget ends."""
    end = None
    while True:
        sample, raw = sampler.get()
        if sample is None:
            time.sleep(0.1)
            continue
        end = end or sample["frame"] + frames
        if sample["rg"] not in (NOT_ENGAGED_GROUP, None):
            return sample
        if sample["frame"] >= end:
            raise TimeoutError(f"opponent did not engage while walking in (group {sample['rg']})")
        gap = abs(sample["rpos"][1] - sample["ppos"][1]) if sample["rpos"] else 0
        if gap >= ENGAGE_RANGE_Q8:
            raise AssertionError(
                f"the player is {gap // 256} map units above or below him: PlayerDistance() counts height, "
                f"so it can never fall under {ENGAGE_RANGE_Q8 // 256} map units on this level")
        step = walk_step(sample)
        if step is None:
            time.sleep(0.3)
            continue
        name, hold = step
        key("keydown", name)
        time.sleep(hold)
        key("keyup", name)
        time.sleep(0.2)


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--name", default="fight", help="run copy and container suffix")
    parser.add_argument("--out", required=True, help="JSON-lines output path")
    parser.add_argument("--mirror", help="second JSON-lines path (another directory) written row by row")
    parser.add_argument("--max-frames", type=int, default=1500, help="frames to fight before stopping")
    parser.add_argument("--gate", choices=("busy", "any"), default="busy", help="when a tap is allowed")
    parser.add_argument("--cheat", default="", help="console cheat to set to 1 before the fight")
    parser.add_argument("--approach", choices=("place", "walk"), default="place",
                        help="if he does not engage: write the player beside him, or walk in with held keys")
    parser.add_argument("--interval", type=float, default=0.2, help="sampler period in seconds")
    args = parser.parse_args()
    image = subprocess.check_output(
        [str(ROOT / "dev/scripts/ensure-retro-runtime.sh")], text=True).strip()
    run = MAGIC / f"retro-run-{args.name}"
    name = f"retro-{args.name}"
    reader = MAGIC / "tools/original-state.py"
    place = MAGIC / "helpers/orig_write_in.py"
    for needed in (reader, place):
        if not needed.exists():
            sys.exit(f"{needed} is missing; restore the local original-game tooling bundle")
    lock = open(Path(args.out).with_suffix(".lock"), "w")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    run_lock = run.with_suffix(".lock").open("w")
    fcntl.flock(run_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    if not run.exists():
        shutil.copytree(MAGIC / "retro-run", run, symlinks=True)
    maps = run / "Redguard/maps"
    if maps.is_symlink() or not maps.resolve().is_relative_to(run.resolve()):
        sys.exit(f"{maps}: original needs run-local writable maps")
    ini = run / "Redguard/WORLD.INI"
    ini.write_text(re.sub(r"(?m)^start_world=\d+", "start_world=30", ini.read_text(errors="surrogateescape")),
                   errors="surrogateescape")
    sh = lambda *c, **k: subprocess.run(c, capture_output=True, text=True, **k)
    sh("docker", "rm", "-f", name)
    sh("docker", "run", "--rm", "--entrypoint", "find", "-v", f"{run}:/game", image,
       "/game/Redguard/maps", "-iname", "*.tsg", "-delete", check=True)
    sh("docker", "run", "-d", "--rm", "--name", name, "--cap-add", "SYS_PTRACE",
       "-v", f"{run}:/game", "-v", f"{MAGIC}/gog:/gog:ro",
       "-v", f"{MAGIC}/gog/extracted/app/game.gog:/game/game.gog:ro", image, check=True)
    sh("docker", "cp", str(reader), f"{name}:/tmp/original-state.py", check=True)
    sh("docker", "cp", str(ROOT / "dev/scripts/original-memory-region.py"),
       f"{name}:/tmp/original-memory-region.py", check=True)
    sh("docker", "cp", str(place), f"{name}:/tmp/orig_write_in.py", check=True)
    key = lambda *c: sh("docker", "exec", "-e", "DISPLAY=:1", name, "xdotool", *c)
    # Every row is fsynced, optionally mirrored to a second directory, and the run is sealed with a
    # manifest at the end; a pod replacement or a cleaned artifacts directory has already removed
    # finished runs before they were read.
    log = DurableLog(args.out, args.mirror)

    def emit(row):
        log.append(row)

    def phase(label):
        sample = sampler.get()[0] if "sampler" in locals() else None
        print(f"[{time.strftime('%H:%M:%S')}] {label} frame={sample and sample['frame']}", flush=True)

    sampler = Sampler(name, args.interval)
    # The player exists before Richton does; wait for a live world, then drive the console.
    wait_frames(sampler, 0, 360, until=lambda s: s["frame"] > 0 and s["ph"])
    wait_frames(sampler, 30, 600)
    shots = Path(args.out).parent / "console-shots"
    shots.mkdir(parents=True, exist_ok=True)

    def shot(tag):
        """Save a screenshot of the original's screen; the console reply is evidence, not a guess."""
        sh("docker", "exec", "-e", "DISPLAY=:1", name, "scrot", "-o", "/tmp/console.png")
        target = shots / f"{tag}.png"
        sh("docker", "cp", f"{name}:/tmp/console.png", str(target))
        return str(target)

    def current_frame():
        raw = sampler.get()[1]
        return ((raw or {}).get("counters") or {}).get("mainloop_frame")

    def press_wall(name_, seconds):
        """Hold a key for wall time; the original's frame counter is frozen while its console is open."""
        key("keydown", name_)
        time.sleep(seconds)
        key("keyup", name_)

    console = Console(
        hold=lambda name_, frames: frame_hold(lambda k: key("keydown", k), lambda k: key("keyup", k),
                                              current_frame, name_, frames),
        press=press_wall,
        console_mode=lambda: (sampler.get()[1] or {}).get("console_mode"),
        pause=time.sleep,
        shot=shot)
    for number, line in enumerate(("item add,79", "getmapmarker 32")):
        typed, answered = console.run(line, f"console-{number}")
        emit({"ev": "console", "line": line, "typed": typed, "answered": answered})
    # `getmapmarker 32` replaces the world with the finale staging scene: the opponent leaves the
    # actor list and returns only when the scripted approach ends (measured: ~2,100 frames later).
    # Placing the player before that moves nothing, so wait for the unload and then the return.
    phase("console done; waiting for the staging scene to unload the opponent")
    wait_frames(sampler, 1500, 900, until=lambda s: s["rg"] is None)
    phase("waiting for the scripted approach to bring the opponent back")
    first = wait_frames(sampler, 0, 1800, until=lambda s: s["rg"] is not None)
    emit({"ev": "opponent-present", **first})
    phase("setting the cheat")
    if args.cheat:
        key("key", "--delay", "200", "F12")
        wait_frames(sampler, 4, 120)
        key("type", "--delay", "100", f"{args.cheat} = 1")
        key("key", "--delay", "300", "KP_Enter")
        wait_frames(sampler, 6, 120)
        key("key", "--delay", "200", "F12")
        wait_frames(sampler, 4, 120)
    # A one-frame press is missed at a few frames per second: hold the key for about 1.5 s of wall
    # time (measured: held `s` reaches the armed idle group, a one-frame press never does).
    phase("drawing the sword")
    key("keydown", "s")
    time.sleep(1.5)
    key("keyup", "s")
    drawn = wait_frames(sampler, 0, 1500, until=lambda s: s["pg"] == ARMED_IDLE_GROUP)
    emit({"ev": "sword-drawn", **drawn})
    phase("sword drawn; waiting for the opponent to engage by himself")
    try:
        engaged = wait_frames(sampler, 900, 900, until=lambda s: s["rg"] != NOT_ENGAGED_GROUP)
    except TimeoutError:
        if args.approach == "walk":
            phase("not engaged; walking in with held keys")
            engaged = walk_in(sampler, key, 3000)
        else:
            phase("not engaged; placing the player beside him")
            sh("docker", "exec", name, "python3", "/tmp/orig_write_in.py", str(OPPONENT_ID), "-1")
            wait_frames(sampler, 8, 120)
            engaged = wait_frames(sampler, 1500, 1500, until=lambda s: s["rg"] != NOT_ENGAGED_GROUP)
    phase("engaged; fighting")
    fight_start = engaged["frame"]
    next_state_frame = fight_start
    while True:
        sample, raw = sampler.get()
        if sample is None:
            time.sleep(0.05)
            continue
        frames = sample["frame"] - fight_start
        if sample["frame"] >= next_state_frame:
            # A periodic state row so the original's range and step-back can be set beside the
            # native fight snapshots (same fields: groups, health, distance, positions).
            emit({"ev": "state", "frames": frames, "dist": horizontal_distance(sample),
                  "pdist": player_distance_q8(sample), **sample})
            next_state_frame = sample["frame"] + STATE_EVERY_FRAMES
        if sample["rh"] == 0 or sample["ph"] == 0 or frames >= args.max_frames:
            emit({"ev": "end", "rh": sample["rh"], "ph": sample["ph"], "frames": frames, "fps": sample["fps"],
                  "reason": "kill" if sample["rh"] == 0 else "player-dead" if sample["ph"] == 0 else "frame-budget"})
            break
        if not ready_to_tap(sample, args.gate):
            time.sleep(0.05)
            continue
        sh("docker", "exec", name, "python3", "/tmp/orig_write_in.py", str(OPPONENT_ID), "-1")  # stay in range, facing him
        key("keydown", "Control_L")
        time.sleep(0.6)  # held for a few frames: a one-frame press is missed at this frame rate
        key("keyup", "Control_L")
        after = []
        mark = sampler.get()[0]
        end_frame = (mark or sample)["frame"] + 14
        while True:
            now = sampler.get()[0]
            if now:
                after.append(now)
                if now["frame"] >= end_frame:
                    break
            time.sleep(0.05)
        emit({"ev": "tap", "before": sample, **classify_tap(sample, after)})
    sampler.stop()
    sh("docker", "rm", "-f", name)
    log.seal("original-fight run complete")
    rows = [json.loads(line) for line in open(args.out)]
    print(json.dumps(summarize(rows), indent=1))
    print(f"sealed {args.out}: {verify(args.out)[1]}", flush=True)


if __name__ == "__main__":
    main()
