#!/usr/bin/env python3
"""Runs inside the retro container: waits for the original's world loop, starts DOSBox-X's own
WAV recorder (Ctrl+Alt+W; it records the mixer output in emulated time), replays a key timeline with
xdotool and logs the game's loop frame at the start, at every event and at the stop.

Usage: original-capture-inner.py <state-reader> <events.json> <seconds-after-start>
events.json: [[seconds, "key"|"keydown"|"keyup", "xdotool-key-name"], ...] relative to the start.
Prints one JSON object: {"start": {"wall": w, "frame": f}, "events": [[t, action, key, frame, wav_bytes]],
"stop": {"wall": w, "frame": f}, "sizes": [[t, bytes]], "shots": [[t_begin, t_end, file, counters]]} (the recording size every 20 ms, to interpolate emulated time between its 64 KiB steps). The recorder writes its WAV into the DOSBox-X capture directory.
"""
import glob
import json
import os
from pathlib import Path
import threading
import subprocess
import sys
import time

reader, events_path, seconds = sys.argv[1], sys.argv[2], float(sys.argv[3])
events = json.load(open(events_path))
env = {**os.environ, "DISPLAY": ":1"}
capture_dir = Path(os.environ.get("CYRUS_CAPTURE_DIR", "/game/capture"))
startup_capture = os.environ.get("CYRUS_CAPTURE_STARTUP") == "1"


def counters():
    """The state reader's counters (mainloop frame, ticks and frames passed), or None."""
    try:
        out = subprocess.run([sys.executable, reader], capture_output=True, text=True, timeout=20).stdout
        return json.loads(out)["counters"]
    except (ValueError, KeyError, subprocess.TimeoutExpired):
        return None


def frame():
    """The game loop's frame counter, read through the state reader (-1 until the world runs)."""
    try:
        out = subprocess.run([sys.executable, reader], capture_output=True, text=True, timeout=20).stdout
        return json.loads(out)["counters"]["mainloop_frame"]
    except (ValueError, KeyError, subprocess.TimeoutExpired):
        return -1


def recorded_bytes():
    """Size of the newest recording: the audio written so far, so the emulated audio clock now."""
    files = sorted(glob.glob(str(capture_dir / "*.wav")), key=os.path.getmtime)
    return os.path.getsize(files[-1]) if files else 0


def toggle_recorder():
    subprocess.run(["xdotool", "keydown", "Control_L", "keydown", "Alt_L", "key", "w",
                    "keyup", "Alt_L", "keyup", "Control_L"], env=env, check=True)


waited = time.time()
if startup_capture:
    while not (capture_dir / "START.OK").exists():
        if time.time() - waited > 15:
            raise SystemExit("startup capture: DOS prompt did not become ready")
        time.sleep(0.05)
else:
    while frame() <= 400:
        if time.time() - waited > float(os.environ.get("CYRUS_FRAME_WAIT", "150")):
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
if os.path.exists("/tmp/clock.py"):
    # An optional game-clock logger (kept outside the repository): one line per game frame with
    # the wall time, so frames can be mapped to screenshots and key events.
    capture_dir.mkdir(exist_ok=True)
    subprocess.Popen([sys.executable, "/tmp/clock.py", reader, str(capture_dir / "clock.log"), str(seconds + 5)])
shots = []
interval = float(os.environ.get("CYRUS_SHOT_INTERVAL", "0"))


def take_shot(index):
    path = capture_dir / "shots" / f"s{index:04d}.png"
    path.parent.mkdir(exist_ok=True)
    began = time.time() - start
    subprocess.run(["scrot", "-o", str(path)], env=env, check=True)
    shots.append([round(began, 3), round(time.time() - start, 3), path.name, counters()])


def take_shots(index):
    while time.time() - start < seconds and not stopped.is_set():
        take_shot(index)
        index += 1
        stopped.wait(interval)


stopped = threading.Event()
launch = None
if startup_capture:
    begin = {"wall": start, "frame": frame(), "wav_bytes": recorded_bytes()}
    if interval > 0:
        take_shot(0)
    subprocess.run(["xdotool", "type", "rgfx.exe"], env=env, check=True)
    launch = round(time.time() - start, 3)
    subprocess.run(["xdotool", "key", "Return"], env=env, check=True)
shot_thread = None
if interval > 0:
    shot_thread = threading.Thread(target=take_shots, args=(len(shots),), daemon=True)
    shot_thread.start()
if not startup_capture:
    time.sleep(1)
    begin = {"wall": start, "frame": frame(), "wav_bytes": recorded_bytes()}
done, log, sizes = 0, [], []
if os.path.exists("/tmp/hook.py"):
    log.append([hook_delay, "hook", "placement", frame(), recorded_bytes()])
next_clock_poll = 0
reason = "duration"
while time.time() - start < seconds:
    now = time.time() - start
    while done < len(events) and events[done][0] <= now:
        _, action, key = events[done]
        subprocess.Popen(["xdotool", action, key], env=env)
        mark = recorded_bytes()
        log.append([round(now, 3), action, key, frame(), mark])
        done += 1
    if startup_capture and now >= next_clock_poll:
        if frame() > 0:
            reason = "world-progress"
            break
        next_clock_poll = now + 0.25
    sizes.append([round(now, 3), recorded_bytes()])
    time.sleep(0.02)
end = {"wall": time.time(), "frame": frame(), "wav_bytes": recorded_bytes()}
stopped.set()
if shot_thread:
    shot_thread.join()
toggle_recorder()
time.sleep(1)
timeline = {"start": begin, "events": log, "stop": end, "sizes": sizes, "shots": shots}
if startup_capture:
    timeline["launch"] = launch
    end["reason"] = reason
print(json.dumps(timeline))
