#!/usr/bin/env python3
"""Check startup capture completion through a real local control socket."""
import json
from pathlib import Path
import socketserver
import subprocess
import sys
import tempfile
import threading
import unittest

SCRIPT = Path(__file__).with_name("native-startup-capture.py")


def state(phase="SelectingInstall", title=False, ready=False, movie=None, seconds=0):
    return {"state": phase, "startup": {"title_visible": title},
            "menu": {"ready": ready, "movie": movie}, "clock": {"game_seconds": seconds}}


class Capture(unittest.TestCase):
    def capture(self, states, expected="menu"):
        index = 0
        screenshots = []

        class Handler(socketserver.StreamRequestHandler):
            def handle(self):
                nonlocal index
                command = json.loads(self.rfile.readline())["cmd"]
                if command["type"] == "state":
                    result = states[min(index, len(states) - 1)]
                    index += 1
                else:
                    screenshots.append(command["path"])
                    result = {}
                self.wfile.write((json.dumps({"ok": True, "result": result}) + "\n").encode())

        with socketserver.TCPServer(("127.0.0.1", 0), Handler) as server:
            worker = threading.Thread(target=server.serve_forever, daemon=True)
            worker.start()
            try:
                with tempfile.TemporaryDirectory() as directory:
                    trace = Path(directory) / "timeline.json"
                    run = subprocess.run(
                        [sys.executable, str(SCRIPT), str(server.server_address[1]), str(trace),
                         "--expect", expected, "--prepare-seconds", "0.2", "--seconds", "0.2",
                         "--interval", "0.01"], capture_output=True, text=True, timeout=3,
                    )
                    record = json.loads(trace.read_text())
            finally:
                server.shutdown()
                worker.join()
        return run, record, screenshots, index

    def test_preparation_screenshot_does_not_make_a_successful_capture(self):
        run, record, shots, _ = self.capture([state()])
        self.assertNotEqual(run.returncode, 0)
        self.assertEqual(record["stop"], "preparation-timeout")
        self.assertTrue(shots)

    def test_title_then_ready_menu_completes(self):
        run, record, _, count = self.capture([
            state(), state("Loading", True), state("Menu", True), state("Menu", True, True),
        ])
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(record["stop"], "menu-ready")
        self.assertEqual(count, 4)

    def test_ready_book_fails_when_introduction_was_expected(self):
        run, record, _, _ = self.capture([
            state("Loading", True), state("Menu", True, True),
        ], "movie")
        self.assertNotEqual(run.returncode, 0)
        self.assertEqual(record["stop"], "unexpected-menu")

    def test_movie_must_present_a_frame_before_success(self):
        run, record, _, count = self.capture([
            state("Loading", True),
            state("Menu", True, movie={"loading": True, "frame": 0}),
            state("Menu", True, movie={"loading": False, "frame": 1}),
        ], "movie")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(record["stop"], "movie-playing")
        self.assertEqual(count, 3)

    def test_world_progress_stops_even_when_another_terminal_was_expected(self):
        run, record, _, count = self.capture([
            state("Loading", True), state("InGame", True, seconds=1), state("InGame", True, seconds=2),
        ], "movie")
        self.assertNotEqual(run.returncode, 0)
        self.assertEqual(record["stop"], "world-progress")
        self.assertEqual(count, 2)

    def test_world_endpoint_requires_progress_not_just_the_state(self):
        run, record, _, count = self.capture([
            state("Loading", True), state("InGame", True), state("InGame", True, seconds=0.1),
        ], "world")
        self.assertEqual(run.returncode, 0, run.stderr)
        self.assertEqual(record["stop"], "world-progress")
        self.assertEqual(count, 3)

    def test_title_alone_times_out_without_completing(self):
        run, record, _, _ = self.capture([state("Loading", True)])
        self.assertNotEqual(run.returncode, 0)
        self.assertEqual(record["stop"], "presentation-timeout")


if __name__ == "__main__":
    unittest.main()
