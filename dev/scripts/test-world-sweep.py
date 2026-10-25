#!/usr/bin/env python3
"""Exercise world-sweep against a fake control socket without loading an install."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("world_sweep", Path(__file__).with_name("world-sweep.py"))
sweep = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sweep)


class WorldSweepTests(unittest.TestCase):
    def run_sweep(self, frames):
        now = [0]
        clocks = iter([1, 2])
        frames = iter(frames)

        class Connection:
            def __enter__(self):
                return self

            def __exit__(self, *args):
                pass

            def sendall(self, request):
                cmd = json.loads(request)["cmd"]
                if cmd["type"] == "state":
                    result = {"state": "InGame", "scene_ready": True, "scene": "START"}
                    # Readiness does not sample game_seconds.
                    result["clock"] = {"game_seconds": now[0]}
                elif cmd["type"] == "scripts":
                    result = {"actors": []}
                    if cmd["limit"] == 1:
                        result["globals"] = {"world_loop_frame": next(frames)}
                else:
                    result = {}
                self.reply = (json.dumps({"ok": True, "result": result}) + "\n").encode()

            def recv(self, size):
                return self.reply

        output = io.StringIO()
        with patch.object(sweep.socket, "create_connection", side_effect=lambda *a, **k: Connection()), \
             patch.object(sweep.time, "time", side_effect=lambda: now[0]), \
             patch.object(sweep.time, "sleep", side_effect=lambda seconds: now.__setitem__(0, now[0] + seconds)), \
             patch.object(sweep.sys, "argv", ["world-sweep.py", "--seconds", "1", "START"]), \
             contextlib.redirect_stdout(output):
            return sweep.main(), output.getvalue()

    def test_host_clock_does_not_hide_a_stalled_script_loop(self):
        result, output = self.run_sweep([0, 0])
        self.assertEqual(result, 1)
        self.assertIn("script world_loop_frame did not advance (0)", output)

    def test_running_script_loop_passes(self):
        result, output = self.run_sweep([3, 4])
        self.assertEqual(result, 0, output)


if __name__ == "__main__":
    unittest.main()
