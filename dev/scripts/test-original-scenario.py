#!/usr/bin/env python3
"""Controls for the original-scenario driver's pure logic, independent of the original and of game bytes."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('scenario', Path(__file__).with_name('original-scenario.py'))
scenario = importlib.util.module_from_spec(spec)
spec.loader.exec_module(scenario)


class RetailEntryDriver(unittest.TestCase):
    def run_driver(self, acknowledge=True, advance_clock=True):
        import tempfile
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "Redguard").mkdir()
            (root / "Redguard/MENU.INI").write_text("[general]\nmovie_keys[0] = 10,20\n")
            class Game:
                run = root
                movie_frame = -1
                frame = 0
                keys = []

                def raw(self):
                    # The allocated movie buffer precedes the decoder handle.
                    if self.movie_frame == -1:
                        self.movie_frame = 0
                        return {"world": 4294967295, "startup": {"movie_active": True, "movie_frame": None}}
                    if acknowledge:
                        self.movie_frame += 1
                    live = self.movie_frame > 25
                    self.frame += int(live)
                    return {"startup": {"movie_active": not live, "movie_frame": self.movie_frame},
                            "world": 0 if live else 4294967295,
                            "counters": {"mainloop_frame": self.frame, "clock_scale": self.frame * 256 if advance_clock else 0,
                                         "world_fade": -100 if self.frame < 3 else 0,
                                         "clock_continuation": {"warmup": 12 if self.frame < 5 else 0}},
                            "actors": [{"selected": True}] if live else []}

                def alive(self):
                    return True

                def hold(self, key, seconds):
                    self.keys.append(key)
                    if acknowledge:
                        self.movie_frame += 10
            game = Game()
            ticks = iter(range(1000))
            result = scenario.enter_retail_world(game, timeout=20, clock=lambda: next(ticks), sleep=lambda _: None)
            return game, result

    def test_no_world_loading_and_movie_checkpoints_reach_advancing_retail_world(self):
        game, result = self.run_driver()
        self.assertEqual(game.keys, ["Escape", "Escape", "Escape"])
        self.assertEqual(result["state"]["world"], 0)
        self.assertEqual(result["events"][-1]["transition"], "world progress")
        self.assertGreaterEqual(result["events"][-2]["frame"], 5)
        self.assertGreater(result["events"][-1]["frame"], result["events"][-2]["frame"])

    def test_frames_without_game_clock_progress_are_not_a_running_world(self):
        with self.assertRaisesRegex(TimeoutError, "world progress.*no acknowledgement"):
            self.run_driver(advance_clock=False)

    def test_unacknowledged_input_stops_before_more_keys(self):
        with self.assertRaisesRegex(TimeoutError, "movie skip.*no acknowledgement"):
            self.run_driver(acknowledge=False)


class ReloadBoundary(unittest.TestCase):
    def sample(self, fade=0, warmup=0):
        return {"world":20,"counters":{"mainloop_frame":40,"world_fade":fade,
                "clock_continuation":{"warmup":warmup}},"actors":[{"selected":True}]}

    def test_reload_waits_for_the_existing_fade_and_its_clock_warmup(self):
        self.assertFalse(scenario.reload_boundary_ready(self.sample(-100, 0)))
        self.assertFalse(scenario.reload_boundary_ready(self.sample(0, 12)))
        self.assertTrue(scenario.reload_boundary_ready(self.sample()))

    def test_unavailable_bindings_are_not_treated_as_a_settled_clock(self):
        self.assertFalse(scenario.reload_boundary_ready(None))
        self.assertFalse(scenario.reload_boundary_ready({"world":-1}))
        raw = self.sample()
        del raw["counters"]["world_fade"]
        with self.assertRaisesRegex(ValueError, "fade.*warmup"):
            scenario.reload_boundary_ready(raw)

    def test_wait_observes_natural_progress_without_writing_the_clock(self):
        game = scenario.Original.__new__(scenario.Original)
        rows = iter([self.sample(-100, 0), self.sample(0, 12), self.sample()])
        game.raw = lambda: next(rows)
        def wait(frames, timeout, until):
            self.assertEqual((frames, timeout), (240, 180))
            self.assertFalse(until({}))
            self.assertFalse(until({}))
            self.assertTrue(until({}))
            return "settled"
        game.wait = wait
        self.assertEqual(game.wait_reload_boundary(), "settled")


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


LOAD_MENU = """
[page0]
text[0] = SAVE
default[0] = 1
action[0] = 1
text[1] = LOAD
action[1] = 2
text[2] = NEW
action[2] = 10
text[3] = OPTIONS
action[3] = 4
[page2]
text[0] = LOAD GAME
text[1] = 12345
action[1] = 202
text[2] = 12345
action[2] = 202
"""


class RetailLoad(unittest.TestCase):
    def test_load_walk_skips_the_disabled_save_item_and_counts_slot_rows(self):
        pages = scenario.menu_pages(LOAD_MENU)
        steps = scenario.book_load_keys(pages, slot_row=1)
        # Save is disabled at startup, so the selection starts on LOAD: no Down before Return.
        self.assertEqual([(a, b) for a, b, _ in steps],
                         [("hold", "Return"), ("hold", "Down"), ("hold", "Return")])
        self.assertTrue(all(settle >= 2 for _, _, settle in steps))

    def test_load_walk_counts_downs_when_the_default_is_not_disabled(self):
        pages = scenario.menu_pages(LOAD_MENU.replace("action[0] = 1", "action[0] = 4"))
        steps = scenario.book_load_keys(pages, slot_row=0, disabled=())
        # The default is now item action 4 (first of two such items), Load follows it: one Down.
        self.assertEqual([b for _, b, _ in steps], ["Down", "Return", "Return"])

    def test_load_walk_refuses_menus_without_the_assumed_actions(self):
        with self.assertRaisesRegex(ValueError, "Load action"):
            scenario.book_load_keys(scenario.menu_pages(LOAD_MENU.replace("action[1] = 2", "action[1] = 9")), 0)
        with self.assertRaisesRegex(ValueError, "202"):
            scenario.book_load_keys(scenario.menu_pages(LOAD_MENU.replace("202", "203")), 0)
        with self.assertRaisesRegex(ValueError, "nonnegative"):
            scenario.book_load_keys(scenario.menu_pages(LOAD_MENU), -1)

    def test_staging_replaces_the_saves_and_the_config_inside_the_run_copy_only(self):
        import tempfile
        root = Path(tempfile.mkdtemp())
        run = root / "run"
        (run / "Redguard/SAVEGAME/SAVEGAME.007").mkdir(parents=True)
        (run / "Redguard/SAVEGAME/SAVEGAME.007/SAVEGAME.SAV").write_bytes(b"old")
        (run / "Redguard/SYSTEM.INI").write_text("[debug]\nfinal_version=0\n")
        pristine = root / "pristine.ini"
        pristine.write_text("[system]\nretail=1\n")
        source = root / "source/SAVEGAME.002"
        source.mkdir(parents=True)
        (source / "SAVEGAME.SAV").write_bytes(b"retail save")
        (source / "START.TSG").write_bytes(b"previous world state")
        (source / "LOGBOOK.TXT").write_text("retained journal")
        (source / "capture.png").write_bytes(b"not a checkpoint sidecar")
        slot = scenario.stage_retail_save(run, pristine, source)
        self.assertEqual(slot.name, "SAVEGAME.000")
        self.assertEqual((slot / "START.TSG").read_bytes(), b"previous world state")
        self.assertEqual((slot / "LOGBOOK.TXT").read_text(), "retained journal")
        self.assertEqual(sorted(p.name for p in slot.iterdir()), ["LOGBOOK.TXT", "SAVEGAME.SAV", "START.TSG"])
        self.assertEqual((slot / "SAVEGAME.SAV").read_bytes(), b"retail save")
        self.assertEqual(sorted(p.name for p in (run / "Redguard/SAVEGAME").iterdir()), ["SAVEGAME.000"])
        self.assertEqual((run / "Redguard/SYSTEM.INI").read_text(), "[system]\nretail=1\n")
        self.assertEqual((source / "SAVEGAME.SAV").read_bytes(), b"retail save")

    def test_invalid_sidecar_rejects_before_replacing_previous_private_state(self):
        import tempfile
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            run = root / 'run'
            old = run / 'Redguard/SAVEGAME/SAVEGAME.007'
            old.mkdir(parents=True)
            (old / 'SAVEGAME.SAV').write_bytes(b'old')
            ini = run / 'Redguard/SYSTEM.INI'
            ini.write_text('old config')
            pristine = root / 'pristine.ini'
            pristine.write_text('new config')
            source = root / 'source'
            source.mkdir()
            (source / 'SAVEGAME.SAV').write_bytes(b'new save')
            (source / 'START.TSG').symlink_to(old / 'SAVEGAME.SAV')
            with self.assertRaisesRegex(ValueError, 'checkpoint file.*run-local|checkpoint file.*regular'):
                scenario.stage_retail_save(run, pristine, source)
            self.assertEqual(ini.read_text(), 'old config')
            self.assertEqual((old / 'SAVEGAME.SAV').read_bytes(), b'old')

    def test_staging_refuses_symlinked_game_links_and_sources_without_a_save(self):
        import tempfile
        root = Path(tempfile.mkdtemp())
        shared = root / "shared";shared.mkdir()
        run = root / "run";(run / "Redguard").mkdir(parents=True)
        (run / "Redguard/SYSTEM.INI").write_text("x")
        (run / "Redguard/SAVEGAME").symlink_to(shared)
        with self.assertRaisesRegex(ValueError, "run-local"):
            scenario.stage_retail_save(run, root / "p.ini", root)
        (run / "Redguard/SAVEGAME").unlink()
        (run / "Redguard/SAVEGAME").mkdir()
        (root / "p.ini").write_text("x")
        with self.assertRaisesRegex(ValueError, "no SAVEGAME.SAV"):
            scenario.stage_retail_save(run, root / "p.ini", root / "empty")


class ObserveRetail(unittest.TestCase):
    def test_observation_stops_when_the_container_exits_and_records_each_shot(self):
        import tempfile

        class Fake:
            def __init__(self):
                self.shots, self.alive_calls = 0, 0

            def raw(self):
                return {"world": 0}

            def frame(self):
                return 5

            def screenshot(self, path):
                self.shots += 1
                return {"ok": True}

            def alive(self):
                self.alive_calls += 1
                return self.alive_calls < 3

        game = Fake()
        rows = scenario.observe_retail(game, tempfile.mkdtemp(), seconds=60, interval=0)
        self.assertEqual(len(rows), 3)
        self.assertEqual([r["alive"] for r in rows], [True, True, False])
        self.assertTrue(all(r["shot"] and r["world"] == 0 for r in rows))


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
