#!/usr/bin/env python3
"""Observe production native startup through its control port, preserving failures."""
import argparse
import json
from pathlib import Path
import socket
import time


def capture(port, output, expected, prepare_seconds, seconds, interval, screenshot_dir):
    started = time.monotonic()
    deadline = started + prepare_seconds
    presented = False
    previous = None
    trace = []
    shots = []
    error = None
    reason = "preparation-timeout"
    success = False

    def request(command):
        with socket.create_connection(("127.0.0.1", port), timeout=1) as connection:
            connection.settimeout(10 if command["type"] == "screenshot" else 1)
            connection.sendall((json.dumps({"id": 1, "cmd": command}) + "\n").encode())
            response = json.loads(connection.makefile("rb").readline())
            if response.get("ok") is not True:
                raise ValueError(f"control rejected {command['type']}: {response}")
            return response["result"]

    while time.monotonic() < deadline:
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
            filename = f"startup-{len(shots):02}.png"
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
            break
        time.sleep(interval)
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
    args = parser.parse_args()
    if min(args.prepare_seconds, args.seconds, args.interval) <= 0:
        parser.error("durations and interval must be positive")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    return not capture(args.port, args.output, args.expect, args.prepare_seconds,
                       args.seconds, args.interval, args.screenshot_dir)


if __name__ == "__main__":
    raise SystemExit(main())
