#!/usr/bin/env python3
"""Runs inside the retro container: waits for the original's world loop, starts DOSBox-X's own
WAV recorder (Ctrl+Alt+W; it records the mixer output in emulated time), replays a key timeline with
xdotool and logs the game's loop frame at the start, at every event and at the stop.

Usage: original-capture-inner.py <state-reader> <events.json> <seconds-after-start>
events.json: [[seconds, "key"|"keydown"|"keyup", "xdotool-key-name"], ...] relative to the start.
Prints one JSON object: {"start": {"wall": w, "frame": f}, "events": [[t, action, key, frame, wav_bytes]],
"stop": {"wall": w, "frame": f}, "sizes": [[t, bytes]], "shots": [[t_begin, t_end, file, counters]]} (the recording size every 20 ms, to interpolate emulated time between its 64 KiB steps). The recorder writes its WAV into the DOSBox-X capture directory.
"""
import atexit
import glob
import importlib.util
import json
import math
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
state_timeout = float(os.environ.get("CYRUS_STATE_TIMEOUT", "20"))
if not math.isfinite(state_timeout) or state_timeout <= 0:
    raise SystemExit("state-reader timeout must be positive")


def counters():
    """The state reader's counters (mainloop frame, ticks and frames passed), or None."""
    try:
        out = subprocess.run([sys.executable, reader], capture_output=True, text=True, timeout=state_timeout).stdout
        return json.loads(out)["counters"]
    except (ValueError, KeyError, subprocess.TimeoutExpired):
        return None


def frame():
    """The game loop's frame counter, read through the state reader (-1 until the world runs)."""
    try:
        out = subprocess.run([sys.executable, reader], capture_output=True, text=True, timeout=state_timeout).stdout
        return json.loads(out)["counters"]["mainloop_frame"]
    except (ValueError, KeyError, subprocess.TimeoutExpired):
        return -1


def game_seconds():
    """Observe the reader's game clock without converting wall time into game time."""
    try:
        out = subprocess.run([sys.executable, reader], capture_output=True, text=True, timeout=state_timeout).stdout
        value = float(json.loads(out)["game_seconds"])
        return value if math.isfinite(value) and value >= 0 else None
    except (ValueError, KeyError, subprocess.TimeoutExpired):
        return None


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
    while frame() <= int(os.environ.get("CYRUS_CAPTURE_MIN_FRAME", "400")):
        if time.time() - waited > float(os.environ.get("CYRUS_FRAME_WAIT", "150")):
            break
        time.sleep(0.5)
required_hook = os.environ.get("CYRUS_REQUIRE_HOOK_ACK") == "1"
if required_hook and not os.path.exists("/tmp/hook.py"):
    raise SystemExit("observed emission capture requires a hook")
toggle_recorder()
start = time.time()
epoch = time.monotonic()
hook_capture = None
hook_delay = float(os.environ.get("CYRUS_HOOK_DELAY", "0"))
if os.path.exists("/tmp/hook.py"):
    # Requested delay is only a stimulus request. The probe must acknowledge
    # the actual sound emission and its own game clock before it can be admitted.
    spec = importlib.util.spec_from_file_location("capture_hook", Path(__file__).with_name("capture-hook.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    hook_capture = module.CaptureHook(
        [sys.executable, "/tmp/hook.py", reader, str(hook_delay)]
        + os.environ.get("CYRUS_HOOK_ARGS", "").split(), capture_dir, epoch)
    atexit.register(hook_capture.close)
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
    log.append([hook_delay, "hook_requested", "unacknowledged", None, None])
next_clock_poll = 0
reason = "duration"
goal = float(os.environ.get("CYRUS_CAPTURE_GAME_SECONDS", "0"))
if not math.isfinite(goal) or goal < 0:
    raise SystemExit("game-clock progress must be nonnegative")
begin_game = game_seconds() if goal else None
last_game = begin_game
while time.time() - start < seconds:
    now = time.time() - start
    hook_status = hook_capture.poll() if hook_capture else None
    if hook_status and hook_status["returncode"] != 0:
        reason = "hook-failed"
        break
    while done < len(events) and events[done][0] <= now:
        _, action, key = events[done]
        subprocess.Popen(["xdotool", action, key], env=env)
        mark = recorded_bytes()
        log.append([round(now, 3), action, key, frame(), mark])
        done += 1
    if goal and now >= next_clock_poll:
        last_game = game_seconds()
        if last_game is not None:
            if begin_game is None:
                begin_game = last_game
            if last_game < begin_game:
                reason = "game-clock-reset"
                break
            if last_game - begin_game >= goal:
                reason = "game-progress"
                break
        next_clock_poll = now + 0.25
    if startup_capture and now >= next_clock_poll:
        if frame() > 0:
            reason = "world-progress"
            break
        next_clock_poll = now + 0.25
    sizes.append([round(now, 3), recorded_bytes()])
    time.sleep(0.02)
ended = time.monotonic()
end = {"wall": time.time(), "frame": None if reason == "hook-failed" else frame(), "wav_bytes": recorded_bytes()}
stopped.set()
if shot_thread:
    shot_thread.join()
toggle_recorder()
time.sleep(1)
timeline = {"clock_domain": "emulated_mixer", "hook_wall_clock": "monotonic", "start": begin, "events": log, "stop": end, "sizes": sizes, "shots": shots}
if startup_capture:
    timeline["launch"] = launch
    end["reason"] = reason
hook_error = None
if goal:
    end.update(reason=reason, game_seconds=last_game, required_game_progress=goal,
               observed_game_progress=None if begin_game is None or last_game is None else last_game-begin_game)
    if reason != "game-progress":
        hook_error = "recording reached wall deadline before required game-clock progress"
if hook_capture:
    try:
        timeline["hook"] = hook_capture.finish(required=required_hook, ended=ended)
    except ValueError as error:
        hook_error = str(error)
        timeline["hook"] = json.loads((capture_dir / "hook-result.json").read_text())
    finally:
        hook_capture.close()
print(json.dumps(timeline))
if hook_error:
    raise SystemExit(hook_error)
