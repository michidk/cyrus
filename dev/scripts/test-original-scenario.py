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


class BookSave(unittest.TestCase):
    def test_book_save_is_escape_f2_new_slot_description_confirm(self):
        steps = scenario.book_save_keys("  kotaro before  ")
        self.assertEqual([(a, b) for a, b, _ in steps],
                         [("hold", "Escape"), ("hold", "F2"), ("hold", "Return"),
                          ("type", "kotaro before"), ("hold", "Return")])
        self.assertTrue(all(settle >= 3 for _, _, settle in steps))

    def test_a_description_must_be_nonempty_ascii_and_at_most_64_characters(self):
        for bad in ("", "   ", "x" * 65, "caf\u00e9"):
            with self.assertRaises(ValueError):
                scenario.book_save_keys(bad)
        self.assertEqual(len(scenario.book_save_keys("x" * 64)), 5)


class StallLogic(unittest.TestCase):
    def test_a_frozen_counter_is_stalled_only_after_the_whole_window(self):
        self.assertFalse(scenario.counter_stalled([], 5))
        self.assertFalse(scenario.counter_stalled([(0, 10), (2, 10)], 5))
        self.assertTrue(scenario.counter_stalled([(0, 10), (3, 10), (6, 10)], 5))
        self.assertFalse(scenario.counter_stalled([(0, 10), (3, 10), (6, 11)], 5))

    def test_wait_returns_advanced_when_frames_pass_and_stalled_when_the_book_freezes_the_loop(self):
        clock = {"t": 0.0}

        def now():
            return clock["t"]

        def sleep(seconds):
            clock["t"] += seconds

        running = lambda: int(clock["t"] * 10)
        self.assertEqual(scenario.wait_frames_or_stall(running, 20, clock=now, sleep=sleep)[0], "advanced")
        clock["t"] = 0.0
        frozen = lambda: 100
        self.assertEqual(scenario.wait_frames_or_stall(frozen, 20, window=5.0, clock=now, sleep=sleep),
                         ("stalled", 100))
        clock["t"] = 0.0
        with self.assertRaises(TimeoutError):
            scenario.wait_frames_or_stall(lambda: None, 5, timeout=3, clock=now, sleep=sleep)


if __name__ == "__main__":
    unittest.main()
