#!/usr/bin/env python3
"""Controls for the original console driver, independent of the game."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import original_console as oc


class Rig:
    """A fake original: tracks console mode, keys pressed and elapsed wall time."""

    def __init__(self, accepts_toggle=True, frozen_frames=True):
        self.mode, self.log, self.accepts, self.shots, self.slept = "closed", [], accepts_toggle, [], 0.0
        self.text = ""

    def toggle(self, key):
        if key == "F12" and self.accepts:
            self.mode = "interactive" if self.mode == "closed" else "closed"

    def hold(self, key):
        self.log.append(key)
        self.toggle(key)

    def press(self, key, seconds):
        self.log.append(key)
        self.toggle(key)
        reverse = {v: k for k, v in oc.SPECIAL.items()}
        if self.mode == "interactive" and key != "F12":
            self.text = "" if key == "Return" else self.text + reverse.get(key, key)

    def console_mode(self):
        return self.mode

    def pause(self, seconds):
        self.slept += seconds

    def shot(self, tag):
        self.shots.append(tag)
        return f"/shots/{tag}.png"

    def console(self, **kw):
        return oc.Console(self.hold, self.press, self.console_mode, self.pause, self.shot, editor_text=lambda: self.text, **kw)


class ConsoleDriver(unittest.TestCase):
    def test_item_add_types_one_held_key_per_character_between_open_and_close(self):
        rig = Rig()
        typed, answered = rig.console().run("item add,79", "item79")
        self.assertEqual(rig.log, ["F12", "i", "t", "e", "m", "space", "a", "d", "d", "comma", "7", "9", "Return", "F12"])
        self.assertEqual((typed, answered), ("/shots/item79-typed.png", "/shots/item79-answered.png"))
        self.assertEqual(rig.mode, "closed")

    def test_an_already_open_console_is_not_toggled_closed(self):
        rig = Rig()
        rig.mode = "interactive"
        rig.console().run("oracle", "o")
        self.assertEqual(rig.log[0], "o")

    def test_a_console_that_never_opens_is_reported_with_the_state_seen(self):
        rig = Rig(accepts_toggle=False)
        with self.assertRaisesRegex(oc.ConsoleError, "open the console: console_mode stayed 'closed'"):
            rig.console(budget_seconds=1.0).run("item add,79", "x")
        self.assertEqual(rig.log, ["F12"])  # nothing was typed into a closed console
        self.assertGreaterEqual(rig.slept, 1.0)  # it waited out its budget in wall time, not frames

    def test_no_step_inside_the_console_waits_on_the_frame_counter(self):
        # The original's frame counter stops while its console is open; the driver is built without
        # any frame-waiting callable, so a hung counter cannot stall it.
        import inspect
        self.assertNotIn("wait_frames", inspect.signature(oc.Console.__init__).parameters)
        rig = Rig()
        rig.console().run("oracle", "o")
        self.assertEqual(rig.mode, "closed")

    def test_overlay_is_not_an_editor_and_never_receives_text(self):
        rig = Rig(accepts_toggle=False)
        rig.mode = "overlay"
        with self.assertRaisesRegex(oc.ConsoleError, "interactive"):
            rig.console(budget_seconds=0.5).run("oracle", "overlay")
        self.assertNotIn("o", rig.log)
        self.assertNotIn("Return", rig.log)

    def test_dropped_character_stops_before_submitting_or_typing_the_rest(self):
        rig = Rig()
        rig.press = lambda key, seconds: rig.log.append(key)
        with self.assertRaisesRegex(oc.ConsoleError, "echo.*'o'"):
            rig.console(budget_seconds=0.5).run("oracle", "lost")
        self.assertEqual(rig.log, ["F12", "o"])

    def test_physical_toggle_uses_resolved_keycode_without_symbol_modifiers(self):
        for code in (96, 120):
            self.assertEqual(oc.physical_key_command("keydown", "F12", lambda _: code),
                             ["xdotool", "keydown", str(code)])
        with self.assertRaisesRegex(oc.ConsoleError, "unmapped"):
            oc.physical_key_command("keyup", "F12", lambda _: 0)

    def test_frame_hold_releases_if_reader_fails(self):
        events = []
        def read():
            if events:
                raise RuntimeError("reader lost")
            return 1
        with self.assertRaisesRegex(RuntimeError, "reader lost"):
            oc.frame_hold(lambda key: events.append("down"), lambda key: events.append("up"),
                          read, "F12", 2)
        self.assertEqual(events, ["down", "up"])

    def test_toggle_release_follows_acknowledgement_and_reader_failure(self):
        for fails in (False, True):
            events = []
            def acknowledged():
                events.append("read")
                if fails:
                    raise RuntimeError("reader lost")
                return True
            def hold():
                oc.acknowledged_hold(lambda key: events.append("down"),
                                     lambda key: events.append("up"), acknowledged, "F12")
            if fails:
                with self.assertRaisesRegex(RuntimeError, "reader lost"):
                    hold()
            else:
                hold()
            self.assertEqual(events, ["down", "read", "up"])

    def test_unacknowledged_toggle_is_released_at_the_bound(self):
        events = []
        with self.assertRaisesRegex(oc.ConsoleError, "no input acknowledgement"):
            oc.acknowledged_hold(lambda key: events.append("down"),
                                 lambda key: events.append("up"), lambda: False, "F12", timeout=0)
        self.assertEqual(events, ["down", "up"])

    def test_missing_editor_binding_refuses_to_type(self):
        rig = Rig()
        console = oc.Console(rig.hold, rig.press, rig.console_mode, rig.pause, rig.shot,
                             editor_text=lambda: None)
        with self.assertRaisesRegex(oc.ConsoleError, "editor is not empty"):
            console.run("oracle", "missing")
        self.assertEqual(rig.log, ["F12"])

    def test_report_handler_must_return_to_editable_prompt_before_close(self):
        rig = Rig()
        original_press = rig.press
        def press(key, seconds):
            if key == "F12":
                self.assertEqual(rig.text, "")
            original_press(key, seconds)
            if key == "Return":
                rig.text = None
        def pause(seconds):
            rig.slept += seconds
            if rig.slept >= 2:
                rig.text = ""
        rig.press, rig.pause = press, pause
        rig.console().run("oracle", "report")
        self.assertEqual(rig.mode, "closed")
        self.assertGreaterEqual(rig.slept, 2)

    def test_plan_lowercases_maps_punctuation_and_refuses_unsafe_input(self):
        self.assertEqual(oc.plan("Item Add,79"), ["i", "t", "e", "m", "space", "a", "d", "d", "comma", "7", "9"])
        self.assertEqual(oc.keysym("="), "equal")
        with self.assertRaisesRegex(oc.ConsoleError, "semicolon"):
            oc.plan("item add,79; item add,80")
        with self.assertRaisesRegex(oc.ConsoleError, "100 characters"):
            oc.plan("a" * 101)
        with self.assertRaisesRegex(oc.ConsoleError, "no key mapping"):
            oc.plan("é")

    def test_frame_hold_releases_after_the_frames_and_after_the_wall_limit(self):
        events, counter = [], iter(range(0, 100))
        held = oc.frame_hold(lambda k: events.append(("down", k)), lambda k: events.append(("up", k)),
                             lambda: next(counter), "s", 3)
        self.assertEqual(events, [("down", "s"), ("up", "s")])
        self.assertGreaterEqual(held, 3)
        events.clear()
        frozen = oc.frame_hold(lambda k: events.append("down"), lambda k: events.append("up"),
                               lambda: 5, "s", 3, wall_limit=0.15)
        self.assertEqual((frozen, events), (0, ["down", "up"]))  # a frozen counter still releases the key


if __name__ == "__main__":
    unittest.main()
