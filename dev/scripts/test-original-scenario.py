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


SYNTHETIC_MENU = """
[page0]
text[0] = SAVE
default[0] = 1
action[0] = 1
text[1] = LOAD
action[1] = 2
[page1]
text[0] = SAVE GAME
text[1] = 1234567890
default[1] = 1
action[1] = 101
text[2] = 1234567890
action[2] = 102
"""


class BookSave(unittest.TestCase):
    def test_menu_pages_read_defaults_and_actions_from_the_ini(self):
        pages = scenario.menu_pages(SYNTHETIC_MENU)
        self.assertEqual(scenario.default_action(pages, 0), 1)
        self.assertEqual(scenario.default_action(pages, 1), 101)
        self.assertIsNone(scenario.default_action(pages, 2))
        self.assertEqual(pages[1][2]["action"], "102")

    def test_book_save_is_escape_default_save_default_new_slot_description_confirm(self):
        pages = scenario.menu_pages(SYNTHETIC_MENU)
        steps = scenario.book_save_keys("  kotaro before  ", pages)
        self.assertEqual([(a, b) for a, b, _ in steps],
                         [("hold", "Escape"), ("hold", "Return"), ("hold", "Return"),
                          ("type", "kotaro before"), ("hold", "Return")])
        self.assertNotIn("F2", [b for _, b, _ in steps])
        self.assertTrue(all(settle >= 3 for _, _, settle in steps))

    def test_the_walk_refuses_a_menu_that_does_not_match_the_assumed_actions(self):
        other = SYNTHETIC_MENU.replace("action[1] = 101", "action[1] = 7")
        with self.assertRaisesRegex(ValueError, "page 1"):
            scenario.book_save_keys("x", scenario.menu_pages(other))
        with self.assertRaisesRegex(ValueError, "page 0"):
            scenario.book_save_keys("x", scenario.menu_pages(SYNTHETIC_MENU.replace("default[0] = 1", "")))

    def test_a_description_must_be_nonempty_ascii_and_at_most_64_characters(self):
        pages = scenario.menu_pages(SYNTHETIC_MENU)
        for bad in ("", "   ", "x" * 65, "caf\u00e9"):
            with self.assertRaises(ValueError):
                scenario.book_save_keys(bad, pages)
        self.assertEqual(len(scenario.book_save_keys("x" * 64, pages)), 5)


PNG = b"\x89PNG\r\n\x1a\n" + b"\0" * 2000


class ScreenCapture(unittest.TestCase):
    def capture(self, behaviour):
        """capture_screen against a fake container: behaviour maps a method to (status, stderr, wrote)."""
        import tempfile
        directory = Path(tempfile.mkdtemp())
        destination = directory / "shot.png"

        def copy(source, target):
            if self.last_wrote is not None:
                Path(target).write_bytes(self.last_wrote)
            return 0 if self.last_wrote is not None else 1

        def run_and_note(argv):
            method = argv[0] if argv[0] != "sh" else "xwd"
            status, stderr, self.last_wrote = behaviour[method]
            return status, stderr

        return scenario.capture_screen(run_and_note, copy, destination), destination

    def test_a_failing_scrot_falls_back_and_keeps_its_stderr(self):
        result, destination = self.capture({"scrot": (1, "Can't open display", None),
                                            "import": (0, "", PNG), "xwd": (1, "x", None)})
        self.assertTrue(result["ok"])
        self.assertEqual(result["method"], "import")
        self.assertEqual(result["attempts"][0]["status"], 1)
        self.assertIn("display", result["attempts"][0]["stderr"])
        self.assertTrue(scenario.valid_image(destination))

    def test_a_zero_exit_that_writes_no_image_is_not_a_success(self):
        result, _ = self.capture({"scrot": (0, "", b"not a png"), "import": (0, "", b""),
                                  "xwd": (0, "", b"\x89PNG\r\n\x1a\n")})
        self.assertFalse(result["ok"])
        self.assertTrue(all(a.get("invalid_image") for a in result["attempts"]))

    def test_every_method_failing_reports_all_attempts(self):
        result, _ = self.capture({"scrot": (1, "a", None), "import": (1, "b", None), "xwd": (1, "c", None)})
        self.assertFalse(result["ok"])
        self.assertEqual([a["method"] for a in result["attempts"]], ["scrot", "import", "xwd"])
        self.assertEqual([a["stderr"] for a in result["attempts"]], ["a", "b", "c"])


class BookFailure(unittest.TestCase):
    def test_a_stopped_container_is_reported_as_the_original_exiting_not_as_a_capture_failure(self):
        failed = {"ok": False, "attempts": [{"method": "scrot", "status": 1, "stderr": "No such container"}]}
        self.assertIn("exited after this key", scenario.book_step_failure(failed, container_up=False))
        self.assertIn("no valid screenshot", scenario.book_step_failure(failed, container_up=True))
        self.assertIsNone(scenario.book_step_failure({"ok": True, "attempts": []}, container_up=False))


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
