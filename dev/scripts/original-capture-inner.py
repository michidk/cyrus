#!/usr/bin/env python3
"""Runs inside the retro container: waits for the original's world loop, starts DOSBox-X's own
WAV recorder (F12+W; it records the mixer output in emulated time), replays a key timeline with
xdotool and logs the game's loop frame at the start, at every event and at the stop.

Usage: original-capture-inner.py <state-reader> <events.json> <seconds-after-start>
events.json: [[seconds, "key"|"keydown"|"keyup", "xdotool-key-name"], ...] relative to the start.
Prints one JSON object: {"start": {"wall": w, "frame": f}, "events": [[t, action, key, frame]],
"stop": {"wall": w, "frame": f}}. The recorder writes its WAV into the DOSBox-X capture directory.
"""
import json
import os
import subprocess
import sys
import time

reader, events_path, seconds = sys.argv[1], sys.argv[2], float(sys.argv[3])
events = json.load(open(events_path))
env = {**os.environ, "DISPLAY": ":1"}


def frame():
    """The game loop's frame counter, read through the state reader (-1 until the world runs)."""
    try:
        out = subprocess.run([sys.executable, reader], capture_output=True, text=True, timeout=20).stdout
        return json.loads(out)["counters"]["mainloop_frame"]
    except (ValueError, KeyError, subprocess.TimeoutExpired):
        return -1


def toggle_recorder():
    subprocess.run(["xdotool", "keydown", "F12", "key", "w", "keyup", "F12"], env=env)


while frame() <= 400:
    time.sleep(0.5)
toggle_recorder()
start = time.time()
begin = {"wall": start, "frame": frame()}
done, log = 0, []
while time.time() - start < seconds:
    now = time.time() - start
    while done < len(events) and events[done][0] <= now:
        _, action, key = events[done]
        subprocess.Popen(["xdotool", action, key], env=env)
        log.append([round(now, 3), action, key, frame()])
        done += 1
    time.sleep(0.05)
end = {"wall": time.time(), "frame": frame()}
toggle_recorder()
time.sleep(1)
print(json.dumps({"start": begin, "events": log, "stop": end}))
