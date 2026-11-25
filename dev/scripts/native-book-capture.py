#!/usr/bin/env python3
"""Record bounded Escape/Continue book visits on an already running native game."""
from harness import control_response
import argparse
import json
import os
from pathlib import Path
import time
import uuid


def capture(port, output, screenshot_dir, seconds=120, interval=0.1, process_pid=None):
    from PIL import Image

    deadline = time.monotonic() + seconds
    record = {"success": False, "stage": "world", "visits": [], "error": None}

    def request(command, *, release=False):
        if process_pid is not None:
            try:
                os.kill(process_pid, 0)
            except ProcessLookupError as error:
                raise ValueError(f"{record['stage']}: game process exited: {process_pid}") from error
        remaining = deadline - time.monotonic()
        if remaining <= 0 and not release:
            raise TimeoutError(f"{record['stage']}: run deadline expired")
        # A release uses the normal budget while the run is live. After expiry,
        # allow only a bounded cleanup attempt so failure cannot strand a held key.
        timeout = min(remaining, 30) if remaining > 0 else 2
        label = " ".join(str(command[key]) for key in ("type", "key", "action") if key in command)
        try:
            response = control_response(port, command, timeout=timeout)
        except OSError as error:
            raise type(error)(f"{record['stage']}: {label}: {error}") from error
        if response.get("ok") is not True:
            raise ValueError(f"{label}: control rejected request: {response}")
        return response["result"]

    def wait_state(predicate):
        last_error = None
        while time.monotonic() < deadline:
            try:
                state = request({"type": "state"})
                problem = (state.get("startup") or {}).get("error") or (state.get("menu") or {}).get("unavailable")
                if problem:
                    raise ValueError(f"{record['stage']}: game reported: {problem}")
                if predicate(state):
                    return state
            except (ConnectionError, TimeoutError) as error:
                last_error = str(error)
            time.sleep(interval)
        raise TimeoutError(f"{record['stage']}: run deadline expired; last control error: {last_error}")

    def key(name):
        before = request({"type": "state"})["frame"]
        try:
            request({"type": "input", "key": name, "action": "press"})
            return wait_state(lambda state: state["frame"] > before)
        finally:
            request({"type": "input", "key": name, "action": "release"}, release=True)

    def screenshot(*, allow_black=False):
        path = screenshot_dir / f"book-{uuid.uuid4().hex}.png"
        request({"type": "screenshot", "path": str(path)})
        while time.monotonic() < deadline:
            try:
                with Image.open(path) as image:
                    rgb = image.convert("RGB")
                    rgb.load()
                    pixels = rgb.tobytes()
                    visible = sum(any(pixel) for pixel in zip(pixels[::3], pixels[1::3], pixels[2::3]))
                    result = {"path": str(path), "pixels": rgb.width * rgb.height,
                              "nonblack_pixels": visible}
                if not visible and not allow_black:
                    raise ValueError(f"{record['stage']}: screenshot is entirely black: {path}")
                return result
            except (OSError, SyntaxError):
                time.sleep(interval)
        raise TimeoutError(f"{record['stage']}: screenshot did not complete before run deadline: {path}")

    try:
        state = wait_state(lambda state: state.get("state") == "InGame" or (
            state.get("scene_ready") and (state.get("startup") or {}).get("title_visible")))
        if state.get("state") != "InGame":
            key("Enter")
        wait_state(lambda state: state.get("state") == "InGame" and
                   (state.get("clock") or {}).get("game_seconds", 0) > 0)
        for index in range(2):
            record["stage"] = f"book-{index + 1}"
            state = key("Escape")
            if state.get("state") != "Menu":
                state = wait_state(lambda state: state.get("state") == "Menu")
            if state["menu"].get("context") != "in_game":
                raise ValueError(f"unexpected book context: {state['menu'].get('context')}")
            # Keep entry diagnostics even if readiness or the settled readback later fails.
            visit = {"first_menu": {"state": state}}
            record["visits"].append(visit)
            visit["first_menu"]["screenshot"] = screenshot(allow_black=True)
            state = wait_state(lambda state: state.get("state") == "Menu" and
                               (state.get("menu") or {}).get("ready"))
            visit["entry"] = state
            visit["screenshot"] = screenshot()
            record["stage"] = f"continue-{index + 1}"
            key("Escape")
            resumed = wait_state(lambda state: state.get("state") == "InGame")
            baseline = resumed["clock"]["game_seconds"]
            visit["resumed"] = wait_state(lambda state: state.get("state") == "InGame" and
                                          state["clock"]["game_seconds"] > baseline)
        record["success"] = True
        record["stage"] = "complete"
    except (OSError, ValueError, KeyError) as error:
        record["error"] = f"{type(error).__name__}: {error}"
    finally:
        output.write_text(json.dumps(record, indent=2) + "\n")
    return record


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("port", type=int)
    parser.add_argument("output", type=Path)
    parser.add_argument("--screenshot-dir", type=Path, required=True)
    parser.add_argument("--seconds", type=float, default=120)
    parser.add_argument("--interval", type=float, default=0.1)
    parser.add_argument("--process-pid", type=int, help="local game process whose lifetime bounds capture")
    args = parser.parse_args()
    if args.process_pid is not None and args.process_pid <= 0:
        parser.error("process-pid must be positive")
    if args.seconds <= 0 or args.interval <= 0:
        parser.error("seconds and interval must be positive")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.screenshot_dir.mkdir(parents=True, exist_ok=True)
    result = capture(args.port, args.output, args.screenshot_dir, args.seconds, args.interval, args.process_pid)
    print(json.dumps(result))
    raise SystemExit(0 if result["success"] else 1)
