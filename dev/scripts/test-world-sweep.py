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
    def run_sweep(self, frames, generations=None, actors=False):
        now = [0]
        frames = iter(frames)
        generations = iter(generations) if generations else None

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
                    result = {
                        "actors": [], "world": 1,
                        "generation": next(generations) if generations else 7,
                        "content_id": "synthetic",
                    }
                    if actors and cmd["limit"] == 64 and cmd["offset"] == 0:
                        result["actors"] = [{"name": "actor", "failure": None}]
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

    def test_missing_script_loop_counter_cannot_pass(self):
        result, output = self.run_sweep([None, None])
        self.assertEqual(result, 1, output)

    def test_world_replacement_cannot_look_like_loop_progress(self):
        result, output = self.run_sweep([3, 4], generations=[7, 7, 7, 8])
        self.assertEqual(result, 1, output)
        self.assertIn("script world identity changed", output)

    def test_actor_pages_cannot_mix_generations(self):
        result, output = self.run_sweep([3, 4], generations=[7, 7, 8, 7, 7, 7], actors=True)
        self.assertEqual(result, 1, output)
        self.assertIn("world identity changed during actor pagination", output)

    def test_running_script_loop_passes(self):
        result, output = self.run_sweep([3, 4])
        self.assertEqual(result, 0, output)


if __name__ == "__main__":
    unittest.main()
