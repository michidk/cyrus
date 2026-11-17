#!/usr/bin/env python3
"""Exercise book roundtrip capture through the public control protocol and CLI."""
import json
from pathlib import Path
import socketserver
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from PIL import Image

SCRIPT = Path(__file__).with_name("native-book-capture.py")


class Capture(unittest.TestCase):
    def run_capture(self, black=False, delay=0, resume=True, context="in_game", advance_frames=True, title=False, unavailable=None, release_delay=0, seconds=1, press_delay=0):
        phase = "Loading" if title else "InGame"
        frame = 0
        clock = 1
        commands = []
        writers = []
        releases = 0

        class Handler(socketserver.StreamRequestHandler):
            def handle(self):
                nonlocal phase, frame, clock, releases
                command = json.loads(self.rfile.readline())["cmd"]
                commands.append(command)
                kind = command["type"]
                if kind == "state":
                    frame += int(advance_frames)
                    if phase == "InGame" and resume:
                        clock += 1
                    result = {"state": phase, "frame": frame,
                              "clock": {"game_seconds": clock},
                              "scene_ready": title, "startup": {"title_visible": title},
                              "menu": {"ready": True, "context": context, "unavailable": unavailable}}
                elif kind == "input":
                    if command["action"] == "release":
                        releases += 1
                        if releases == 1:
                            time.sleep(release_delay)
                    if command["action"] == "press":
                        time.sleep(press_delay)
                        phase = "Menu" if phase == "InGame" else "InGame"
                    result = {}
                elif kind == "screenshot":
                    def write():
                        time.sleep(delay)
                        Image.new("RGB", (8, 8), "black" if black else "gold").save(command["path"])
                    writer = threading.Thread(target=write)
                    writers.append(writer)
                    writer.start()
                    result = {}
                else:
                    raise AssertionError(command)
                try:
                    self.wfile.write((json.dumps({"id": 1, "ok": True, "result": result}) + "\n").encode())
                except BrokenPipeError:
                    # Deadline tests deliberately close before the delayed response.
                    pass

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with socketserver.TCPServer(("127.0.0.1", 0), Handler) as server:
                thread = threading.Thread(target=server.serve_forever)
                thread.start()
                try:
                    run = subprocess.run([sys.executable, str(SCRIPT), str(server.server_address[1]),
                                          str(root / "receipt.json"), "--screenshot-dir", str(root),
                                          "--seconds", str(seconds), "--interval", "0.01"],
                                         text=True, capture_output=True, timeout=max(5, seconds + 2))
                finally:
                    server.shutdown()
                    thread.join()
                    for writer in writers:
                        writer.join()
            receipt = json.loads((root / "receipt.json").read_text())
        return run, receipt, commands

    def test_two_visits_wait_for_written_pngs_and_clock_progress(self):
        run, receipt, commands = self.run_capture(delay=0.1)
        self.assertEqual(run.returncode, 0, run.stderr + run.stdout)
        self.assertTrue(receipt["success"])
        self.assertEqual(len(receipt["visits"]), 2)
        self.assertEqual([v["screenshot"]["nonblack_pixels"] for v in receipt["visits"]], [64, 64])
        self.assertEqual([c["action"] for c in commands if c["type"] == "input"],
                         ["press", "release"] * 4)
        self.assertEqual(len({v["screenshot"]["path"] for v in receipt["visits"]}), 2)

    def test_prepared_title_is_dismissed_before_gameplay_entry(self):
        run, receipt, commands = self.run_capture(title=True)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertTrue(receipt["success"])
        inputs = [c for c in commands if c["type"] == "input"]
        self.assertEqual([c["key"] for c in inputs[:2]], ["Enter", "Enter"])
        self.assertEqual(len(receipt["visits"]), 2)

    def test_busy_release_uses_remaining_run_budget(self):
        run, receipt, _ = self.run_capture(release_delay=2.2, seconds=5)
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        self.assertTrue(receipt["success"])
        self.assertEqual(len(receipt["visits"]), 2)

    def test_press_acknowledgement_timeout_still_attempts_release(self):
        run, receipt, commands = self.run_capture(press_delay=0.75, seconds=0.5)
        self.assertEqual(run.returncode, 1)
        self.assertIn("input Escape press", receipt["error"])
        self.assertEqual([c["action"] for c in commands if c["type"] == "input"],
                         ["press", "release"])

    def test_black_book_fails_and_preserves_the_stage(self):
        run, receipt, _ = self.run_capture(black=True)
        self.assertEqual(run.returncode, 1)
        self.assertFalse(receipt["success"])
        self.assertEqual(receipt["stage"], "book-1")
        self.assertIn("entirely black", receipt["error"])

    def test_continue_without_progress_fails_with_receipt(self):
        run, receipt, _ = self.run_capture(resume=False)
        self.assertEqual(run.returncode, 1)
        self.assertFalse(receipt["success"])
        self.assertEqual(receipt["stage"], "continue-1")
        self.assertIn("deadline", receipt["error"])

    def test_deadline_while_key_is_held_still_releases_it(self):
        run, receipt, commands = self.run_capture(advance_frames=False)
        self.assertEqual(run.returncode, 1)
        self.assertIn("deadline", receipt["error"])
        self.assertEqual([c["action"] for c in commands if c["type"] == "input"],
                         ["press", "release"])

    def test_unfinished_png_fails_with_its_path_and_stage(self):
        run, receipt, _ = self.run_capture(delay=1.2)
        self.assertEqual(run.returncode, 1)
        self.assertEqual(receipt["stage"], "book-1")
        self.assertIn("screenshot did not complete", receipt["error"])
        self.assertIn(".png", receipt["error"])

    def test_game_error_is_retained_instead_of_becoming_a_deadline(self):
        run, receipt, _ = self.run_capture(unavailable="menu cache could not load")
        self.assertEqual(run.returncode, 1)
        self.assertIn("game reported: menu cache could not load", receipt["error"])

    def test_startup_book_cannot_substitute_for_gameplay_entry(self):
        run, receipt, _ = self.run_capture(context="startup")
        self.assertEqual(run.returncode, 1)
        self.assertIn("unexpected book context", receipt["error"])


if __name__ == "__main__":
    unittest.main()
