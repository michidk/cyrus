#!/usr/bin/env python3
"""Observe production native startup through its control port, preserving failures."""
import argparse
import json
import os
import uuid
from pathlib import Path
import socket
import time


def capture(port, output, expected, prepare_seconds, seconds, interval, screenshot_dir, host_screenshot_dir, process_pid=None):
    started = time.monotonic()
    deadline = started + prepare_seconds
    presented = False
    previous = None
    trace = []
    shots = []
    error = None
    reason = "preparation-timeout"
    success = False

    def check_process():
        if process_pid is not None:
            try:
                os.kill(process_pid, 0)
            except ProcessLookupError as failure:
                raise ValueError(f"game process exited: {process_pid}") from failure

    def request(command):
        check_process()
        with socket.create_connection(("127.0.0.1", port), timeout=1) as connection:
            connection.settimeout(10 if command["type"] == "screenshot" else 1)
            connection.sendall((json.dumps({"id": 1, "cmd": command}) + "\n").encode())
            response = json.loads(connection.makefile("rb").readline())
            if response.get("ok") is not True:
                raise ValueError(f"control rejected {command['type']}: {response}")
            return response["result"]

    while time.monotonic() < deadline:
        try:
            check_process()
        except ValueError as failure:
            reason = "game-exited"
            error = str(failure)
            break
        try:
            state = request({"type": "state"})
            if not isinstance(state, dict):
                raise ValueError("control state is not an object")
        except (OSError, ValueError, KeyError) as failure:
            error = str(failure)
            time.sleep(interval)
            continue
        now = time.monotonic()
        startup = state.get("startup") or {}
        menu = state.get("menu") or {}
        movie = menu.get("movie") or {}
        title = bool(startup.get("title_visible"))
        phase = state.get("state")
        trace.append({"wall": now - started, "state": state})
        if title and not presented:
            presented = True
            deadline = now + seconds
            reason = "presentation-timeout"
        if phase == "InGame" and (state.get("clock") or {}).get("game_seconds", 0) > 0:
            reason = "world-progress"
            success = presented and expected == "world"
            break
        event = (phase, title, bool(movie), movie.get("loading"), bool(menu.get("ready")))
        if event != previous:
            filename = f"startup-{uuid.uuid4().hex}.png"
            try:
                request({"type": "screenshot", "path": f"{screenshot_dir}/{filename}"})
                shots.append({"wall": now - started, "path": filename})
            except (OSError, ValueError, KeyError) as failure:
                error = f"screenshot: {failure}"
                reason = "screenshot-failed"
                break
            previous = event
        if startup.get("error") or menu.get("unavailable"):
            reason = "startup-error"
            error = startup.get("error") or menu.get("unavailable")
            break
        if phase == "Menu" and movie.get("frame", 0) > 0 and not movie.get("loading", True):
            if expected == "movie":
                reason = "movie-playing"
                success = presented
                break
        if phase == "Menu" and not movie and menu.get("ready"):
            reason = "menu-ready" if expected == "menu" else "unexpected-menu"
            success = presented and expected == "menu"
            if success:
                from PIL import Image
                error = None
                image_path = host_screenshot_dir / shots[-1]["path"]
                while time.monotonic() < deadline:
                    try:
                        with Image.open(image_path) as image:
                            rgb = image.convert("RGB")
                            rgb.load()
                            visible = rgb.getbbox() is not None
                        break
                    except (OSError, ValueError):
                        time.sleep(interval)
                else:
                    visible = False
                    error = f"menu screenshot not completed: {image_path}"
                if not visible:
                    reason = "menu-raster-failed"
                    error = error or f"menu screenshot is entirely black: {image_path}"
                    success = False
            break
        time.sleep(interval)
    if success:
        try:
            check_process()
        except ValueError as failure:
            success = False
            reason = "game-exited"
            error = str(failure)
    record = {"success": success, "expected": expected, "stop": reason,
              "elapsed": time.monotonic() - started, "last_error": error,
              "shots": shots, "samples": trace}
    output.write_text(json.dumps(record, indent=2) + "\n")
    print(f"{reason}: {len(trace)} samples, {len(shots)} screenshots; {output}")
    return success


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("port", type=int)
    parser.add_argument("output", type=Path)
    parser.add_argument("--expect", choices=("menu", "movie", "world"), required=True)
    parser.add_argument("--prepare-seconds", type=float, default=180)
    parser.add_argument("--seconds", type=float, default=45)
    parser.add_argument("--interval", type=float, default=0.15)
    parser.add_argument("--screenshot-dir", default="/out")
    parser.add_argument("--host-screenshot-dir", type=Path, help="host mount of screenshot-dir; required for menu raster acceptance")
    parser.add_argument("--process-pid", type=int, help="reject captures after the owning game exits")
    args = parser.parse_args()
    if args.expect == "menu":
        if args.host_screenshot_dir is None:
            parser.error("--expect menu requires --host-screenshot-dir and Pillow")
        try:
            from PIL import Image  # noqa: F401: validate the raster-check dependency before capture.
        except ImportError:
            parser.error("--expect menu requires Pillow in the observer Python environment")
    if min(args.prepare_seconds, args.seconds, args.interval) <= 0:
        parser.error("durations and interval must be positive")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    return not capture(args.port, args.output, args.expect, args.prepare_seconds,
                       args.seconds, args.interval, args.screenshot_dir, args.host_screenshot_dir, args.process_pid)


if __name__ == "__main__":
    raise SystemExit(main())
