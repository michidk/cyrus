#!/usr/bin/env python3
"""Runs inside the retro container: waits for the original's world loop, starts DOSBox-X's own
WAV recorder (F12+W; it records the mixer output in emulated time), replays a key timeline with
xdotool and logs the game's loop frame at the start, at every event and at the stop.

Usage: original-capture-inner.py <state-reader> <events.json> <seconds-after-start>
events.json: [[seconds, "key"|"keydown"|"keyup", "xdotool-key-name"], ...] relative to the start.
Prints one JSON object: {"start": {"wall": w, "frame": f}, "events": [[t, action, key, frame, wav_bytes]],
"stop": {"wall": w, "frame": f}, "sizes": [[t, bytes]], "shots": [[t_begin, t_end, file]]} (the recording size every 20 ms, to interpolate emulated time between its 64 KiB steps). The recorder writes its WAV into the DOSBox-X capture directory.
"""
import glob
import json
import os
import threading
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


def recorded_bytes():
    """Size of the newest recording: the audio written so far, so the emulated audio clock now."""
    files = sorted(glob.glob("/game/capture/*.wav"), key=os.path.getmtime)
    return os.path.getsize(files[-1]) if files else 0


def toggle_recorder():
    subprocess.run(["xdotool", "keydown", "F12", "key", "w", "keyup", "F12"], env=env)


waited = time.time()
while frame() <= 400:
    if time.time() - waited > float(os.environ.get("CYRUS_FRAME_WAIT", "150")):  # the reader can lose the engine; start anyway (frames read -1)
        break
    time.sleep(0.5)
toggle_recorder()
start = time.time()
hook_delay = float(os.environ.get("CYRUS_HOOK_DELAY", "0"))
if os.path.exists("/tmp/hook.py"):
    # An optional placement hook (kept outside the repository; it may use engine internals). It
    # starts with the recorder and waits CYRUS_HOOK_DELAY seconds, so the stimulus it causes has a
    # known time on the recording's clock (script time == hook_delay).
    subprocess.Popen([sys.executable, "/tmp/hook.py", reader, str(hook_delay)] + os.environ.get("CYRUS_HOOK_ARGS", "").split())
shots = []
interval = float(os.environ.get("CYRUS_SHOT_INTERVAL", "0"))


def take_shots():
    os.makedirs("/game/capture/shots", exist_ok=True)
    index = 0
    while time.time() - start < seconds:
        name = f"/game/capture/shots/s{index:04d}.png"
        began = time.time() - start
        subprocess.run(["scrot", "-o", name], env=env)
        shots.append([round(began, 3), round(time.time() - start, 3), name.rsplit("/", 1)[1]])
        index += 1
        time.sleep(interval)


if interval > 0:
    threading.Thread(target=take_shots, daemon=True).start()
time.sleep(1)
begin = {"wall": start, "frame": frame(), "wav_bytes": recorded_bytes()}
done, log, sizes = 0, [], []
if os.path.exists("/tmp/hook.py"):
    log.append([hook_delay, "hook", "placement", frame(), recorded_bytes()])
while time.time() - start < seconds:
    now = time.time() - start
    while done < len(events) and events[done][0] <= now:
        _, action, key = events[done]
        subprocess.Popen(["xdotool", action, key], env=env)
        mark = recorded_bytes()
        log.append([round(now, 3), action, key, frame(), mark])
        done += 1
    sizes.append([round(now, 3), recorded_bytes()])
    time.sleep(0.02)
end = {"wall": time.time(), "frame": frame(), "wav_bytes": recorded_bytes()}
toggle_recorder()
time.sleep(1)
print(json.dumps({"start": begin, "events": log, "stop": end, "sizes": sizes, "shots": shots}))
