#!/usr/bin/env python3
"""Controls for the original-scenario driver's pure logic, independent of the original and of game bytes."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('scenario', Path(__file__).with_name('original-scenario.py'))
scenario = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scenario)


class PromptProjection(unittest.TestCase):
    def test_a_pending_prompt_projects_text_owner_and_holdoff(self):
        raw = {"prompts": {"pending_text": "TALK TO MAN", "pending_pointer": 4096, "owner_id": 12,
                           "owner_pointer": 8192, "holdoff": 0}}
        self.assertEqual(scenario.project_prompt(raw), {"text": "TALK TO MAN", "owner_id": 12, "holdoff": 0})

    def test_no_pending_prompt_projects_none_text_and_missing_state_projects_none(self):
        raw = {"prompts": {"pending_text": None, "owner_id": None, "holdoff": 22}}
        self.assertEqual(scenario.project_prompt(raw), {"text": None, "owner_id": None, "holdoff": 22})
        self.assertIsNone(scenario.project_prompt({"prompts": None}))
        self.assertIsNone(scenario.project_prompt(None))

    def test_windows_count_inclusive_frames_per_run_of_equal_text(self):
        rows = [{"frame": f, "text": t} for f, t in [
            (1, None), (2, "TALK TO MAN"), (3, "TALK TO MAN"), (4, "TALK TO MAN"), (5, None),
            (6, "ENTER"), (7, "TALK TO MAN"), (8, "TALK TO MAN")]]
        windows = scenario.prompt_windows(rows)
        self.assertEqual([(w["text"], w["first_frame"], w["last_frame"], w["frames"]) for w in windows],
                         [("TALK TO MAN", 2, 4, 3), ("ENTER", 6, 6, 1), ("TALK TO MAN", 7, 8, 2)])
        self.assertEqual(scenario.prompt_windows([]), [])


if __name__ == "__main__":
    unittest.main()
