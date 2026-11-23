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

    def toggle(self, key):
        if key == "F12" and self.accepts:
            self.mode = "open" if self.mode == "closed" else "closed"

    def hold(self, key, frames):
        self.log.append(key)
        self.toggle(key)

    def press(self, key, seconds):
        self.log.append(key)
        self.toggle(key)

    def console_mode(self):
        return self.mode

    def pause(self, seconds):
        self.slept += seconds

    def shot(self, tag):
        self.shots.append(tag)
        return f"/shots/{tag}.png"

    def console(self, **kw):
        return oc.Console(self.hold, self.press, self.console_mode, self.pause, self.shot, **kw)


class ConsoleDriver(unittest.TestCase):
    def test_item_add_types_one_held_key_per_character_between_open_and_close(self):
        rig = Rig()
        typed, answered = rig.console().run("item add,79", "item79")
        self.assertEqual(rig.log, ["F12", "i", "t", "e", "m", "space", "a", "d", "d", "comma", "7", "9", "Return", "F12"])
        self.assertEqual((typed, answered), ("/shots/item79-typed.png", "/shots/item79-answered.png"))
        self.assertEqual(rig.mode, "closed")

    def test_an_already_open_console_is_not_toggled_closed(self):
        rig = Rig()
        rig.mode = "open"
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
        self.assertGreater(rig.slept, 0.0)

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
