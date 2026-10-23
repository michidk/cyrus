#!/usr/bin/env python3
"""Compare a native/browser START duel log with an original-game duel log.

Inputs are the stdout of start-duel.py (cyrus) and original-duel.py (original); both print the
same sample schema: seconds since the bot started, player health, pirate health and, after the
last pirate dies, the story events up to the world change. Pirates are matched by their starting
health (30 and 50). The animation groups seen at the samples are listed for both runs (not a check: sampling is
coarse). Checks (exit 1 if one fails):

  * both runs defeat both pirates;
  * every pirate health step is a whole number of 10-point hits in both runs (a hit is 10);
  * both runs reach the next world after the duel, the player arrives within 300 map units
    (horizontal) and 70 heading units (of 2048) of the same place, and the delay from the last
    pirate's death to the world change agrees within a factor of two. Both delays are in game seconds: the original's from frames over
    its measured frame rate, cyrus's from the sum of its simulation clock's frame scales (a host
    below the minimum frame rate runs the game slower than wall time in both).

    dev/scripts/compare-duel.py ORIGINAL.log NATIVE.log
"""
import json
import sys


def load(path):
    samples, events, defeated = [], [], None
    for line in open(path):
        line = line.strip()
        if line.startswith("both pirates defeated"):
            defeated = json.loads(line[len("both pirates defeated"):])
        elif line.startswith("{"):
            row = json.loads(line)
            (events if "event" in row else samples).append(row)
    return samples, events, defeated


def summarize(path):
    samples, events, defeated = load(path)
    if not samples:
        sys.exit(f"{path}: no samples")
    clock = lambda row: row["game_s"]  # each engine's own simulated seconds, not wall time
    by_start = {}
    for index in range(len(samples[0]["pirates"])):
        start = samples[0]["pirates"][index][1]
        by_start[start] = [row["pirates"][index][1] for row in samples]
    # Damage steps between samples: whole hits of 10 (coarse sampling can join two hits into 20).
    steps = {hp: sorted({a - b for a, b in zip(track, track[1:]) if a != b}) for hp, track in by_start.items()}
    change = next((e for e in events if e["event"] == "world change"), None)
    return {
        "defeated": defeated is not None,
        "player_start": samples[0]["player"],
        "player_end": samples[-1]["player"],
        "duel_s": clock(defeated) if defeated else None,
        "kill_order": [hp for hp, _ in sorted(((hp, next((clock(r) for r, v in zip(samples, track) if v == 0), None)) for hp, track in by_start.items()), key=lambda x: x[1] or 1e9)],
        "steps": steps,
        "delay_s": round(clock(change) - clock(defeated), 1) if change and defeated else None,
        "groups": {"player": sorted({r["groups"]["player"] for r in samples if r.get("groups") and r["groups"]["player"] is not None}),
                   "pirates": sorted({g for r in samples if r.get("groups") for g in r["groups"]["pirates"] if g is not None})},
        "arrival": next(({"position": e["position"], "yaw": e["yaw"]} for e in events if e["event"] == "arrival"), None),
    }


def main():
    original, native = summarize(sys.argv[1]), summarize(sys.argv[2])
    for key in ("defeated", "player_start", "player_end", "duel_s", "kill_order", "steps", "delay_s", "groups", "arrival"):
        print(f"{key:13} original={original[key]}  native={native[key]}")
    failures = []
    if not (original["defeated"] and native["defeated"]):
        failures.append("a run did not defeat both pirates")
    for label, run in (("original", original), ("native", native)):
        if any(step <= 0 or step % 10 for steps in run["steps"].values() for step in steps):
            failures.append(f"{label} pirate health did not fall in whole hits of 10")
    if original["delay_s"] is None or native["delay_s"] is None:
        failures.append("a run did not reach the next world")
    elif not 0.5 <= native["delay_s"] / original["delay_s"] <= 2:
        failures.append("story delay after the duel differs by more than a factor of two")
    arrivals = original["arrival"], native["arrival"]
    if None in arrivals:
        failures.append("a run did not report the arrival state")
    else:
        horizontal = max(abs(a - b) for a, b in zip(arrivals[0]["position"][::2], arrivals[1]["position"][::2]))
        turn = abs((arrivals[0]["yaw"] - arrivals[1]["yaw"] + 1024) % 2048 - 1024)
        print(f"arrival       horizontal offset {horizontal} map units, heading difference {turn} of 2048")
        if horizontal > 300 or turn > 70:
            failures.append("arrival position or heading differs")
    print("PASS" if not failures else "FAIL: " + "; ".join(failures))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
