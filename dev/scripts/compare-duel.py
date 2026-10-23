#!/usr/bin/env python3
"""Compare a native/browser START duel log with an original-game duel log.

Inputs are the stdout of start-duel.py (cyrus) and original-duel.py (original); both print the
same sample schema: seconds since the bot started, player health, pirate health and, after the
last pirate dies, the story events up to the world change. Pirates are matched by their starting
health (30 and 50). Checks (exit 1 if one fails):

  * both runs defeat both pirates;
  * every health step of a pirate is the same 10 points in both runs (damage per player hit);
  * both runs reach the next world after the duel, and the delay from the last pirate's death to
    the world change agrees within a factor of two. The original's delay is in its own game
    seconds (frames over its measured fps; the emulator runs slower than wall time here); cyrus's
    is wall seconds, which its clock follows.

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


def summarize(path, game_clock):
    samples, events, defeated = load(path)
    if not samples:
        sys.exit(f"{path}: no samples")
    clock = (lambda row: row.get("game_s", row["t"])) if game_clock else (lambda row: row["t"])
    by_start = {}
    for index in range(len(samples[0]["pirates"])):
        start = samples[0]["pirates"][index][1]
        by_start[start] = [row["pirates"][index][1] for row in samples]
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
    }


def main():
    original, native = summarize(sys.argv[1], True), summarize(sys.argv[2], False)
    for key in ("defeated", "player_start", "player_end", "duel_s", "kill_order", "steps", "delay_s"):
        print(f"{key:13} original={original[key]}  native={native[key]}")
    failures = []
    if not (original["defeated"] and native["defeated"]):
        failures.append("a run did not defeat both pirates")
    if original["steps"] != native["steps"]:
        failures.append("pirate damage steps differ")
    if original["delay_s"] is None or native["delay_s"] is None:
        failures.append("a run did not reach the next world")
    elif not 0.5 <= native["delay_s"] / original["delay_s"] <= 2:
        failures.append("story delay after the duel differs by more than a factor of two")
    print("PASS" if not failures else "FAIL: " + "; ".join(failures))
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
