#!/usr/bin/env python3
"""Failure controls for the held-jump receipt comparison; no install required."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("held_jump", Path(__file__).with_name("native-held-jump.py"))
held = importlib.util.module_from_spec(spec)
spec.loader.exec_module(held)


def receipts():
    observations = [{"frame": 10 + i, "clock_scale": i * 512,
                     "input_state": {"space": 0 < i <= 1},
                     "player": {"selected": True, "yaw": 0, "position_q8": [0, 0, 0], "group": 0,
                                "health": 100, "falling": 0, "queued": 0, "sliding": 0,
                                "living": 0, "rope_attached": False}} for i in range(3)]
    original = {"key": "space", "normal_frame_rate": 12, "observations": observations,
                "steps": [{"frame": 10 + i, "frame_scale": 512,
                           "input_state": {"space": i < 1}} for i in range(2)],
                "input_edges": [dict(copy.deepcopy(observations[1]), input_state={"space": False})],
                "boundaries": [dict(copy.deepcopy(observations[0]), input_state={"space": True}),
                               copy.deepcopy(observations[-1])]}
    boundaries = []
    for i in range(4):
        scale = max(0, i - 1) * 512 + 1024
        if i == 0:
            scale -= 512
        boundaries.append({"after_updates": i, "update": 20 + i, "generation": 1,
                           "normal_frame_rate": 12, "elapsed_scale": scale,
                           "game_seconds": scale / 3072, "key": "Space" if i in (1, 2) else None,
                           "action": "press" if i == 1 else "release" if i == 2 else None,
                           "player": {"actors": [{"source": "player", "name": "CYRUS",
                              "attributes": [0, 0, 0, 100], "animation": {"current_group": 0},
                              "spatial": {"effective": {"position": [0, 0, 0], "angles": [0, 0, 0]}},
                              "control": {"falling_state": 0, "queued_motion": 0, "sliding_state": 0,
                                          "living_state": 0, "rope_attached": False, "stable_position_valid": True}}]}})
    return original, {"boundaries": boundaries}


class Contracts(unittest.TestCase):
    def test_complete_receipts_match(self):
        original, native = receipts()
        self.assertEqual(held.compare(original, native, 2)["matched_boundaries"], 3)

    def test_incomplete_native_is_rejected(self):
        original, native = receipts()
        native["boundaries"].pop()
        with self.assertRaisesRegex(ValueError, "incomplete native"):
            held.compare(original, native, 2)

    def test_changed_position_is_rejected(self):
        original, native = receipts()
        native["boundaries"][2]["player"]["actors"][0]["spatial"]["effective"]["position"][1] = 1
        with self.assertRaisesRegex(ValueError, "state diverged"):
            held.compare(original, native, 2)

    def test_changed_boundary_clock_and_input_are_rejected(self):
        for field, value in [("update", 99), ("generation", 2), ("elapsed_scale", 999),
                             ("normal_frame_rate", 6), ("action", "press")]:
            original, native = receipts()
            native["boundaries"][2][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                held.compare(original, native, 2)

    def test_incomplete_original_or_release_is_rejected(self):
        for damage in (lambda o: o["steps"].pop(), lambda o: o["input_edges"].clear(),
                       lambda o: o["observations"][1].update(clock_scale=1)):
            original, native = receipts()
            damage(original)
            with self.assertRaises(ValueError):
                held.compare(original, native, 2)

    def test_public_cli_removes_stale_acceptance_and_retains_failed_raw(self):
        import json
        import subprocess
        import sys
        import tempfile
        original, native = receipts()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, captured, output = (root / name for name in ("original.json", "native.json", "accepted.json"))
            source.write_text(json.dumps(original))
            captured.write_text(json.dumps(native))
            command = [sys.executable, str(Path(held.__file__)), "--original", str(source),
                       "--native", str(captured), "--updates", "2", "--out", str(output)]
            subprocess.run(command, check=True, capture_output=True)
            self.assertEqual(json.loads(output.read_text())["matched_boundaries"], 3)
            native["boundaries"].pop()
            captured.write_text(json.dumps(native))
            failed = subprocess.run(command, capture_output=True, text=True)
            self.assertNotEqual(failed.returncode, 0)
            self.assertIn("incomplete native", failed.stderr)
            self.assertFalse(output.exists())
            self.assertEqual(json.loads(output.with_suffix(".raw.json").read_text()), native)
            original_bytes = source.read_bytes()
            alias = command[:-1] + [str(source)]
            self.assertNotEqual(subprocess.run(alias, capture_output=True).returncode, 0)
            self.assertEqual(source.read_bytes(), original_bytes)

    def test_capture_admits_balanced_edges_and_releases_on_failure(self):
        from unittest.mock import patch
        original, native = receipts()
        for failure in (False, True):
            calls = []
            def call(addr, command):
                calls.append(command)
                if command["type"] == "schedule_input":
                    if failure:
                        raise RuntimeError("deadline expired")
                    return native
            with patch.object(held.trace, "wait_live"), patch.object(held.trace, "call", side_effect=call):
                if failure:
                    with self.assertRaisesRegex(RuntimeError, "deadline"):
                        held.capture("unused", original, 2)
                else:
                    self.assertEqual(held.capture("unused", original, 2), native)
            self.assertEqual(calls[0]["events"], [
                {"after_updates": 0}, {"after_updates": 1, "key": "Space", "action": "press"},
                {"after_updates": 2, "key": "Space", "action": "release"}, {"after_updates": 3}])
            self.assertEqual(calls[-1], {"type": "input", "key": "Space", "action": "release"})

    def test_long_capture_rejected_before_taking_input(self):
        original, _ = receipts()
        with self.assertRaisesRegex(ValueError, "2..32"):
            held.capture("unused:1", original, 48)


if __name__ == "__main__":
    unittest.main()
