#!/usr/bin/env python3
"""Controls for the original console driver, independent of the game."""
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import original_console as oc


class Rig:
    """A fake original: tracks console mode, keys held and a frame counter."""

    def __init__(self, accepts_toggle=True):
        self.mode, self.frame, self.log, self.accepts = "closed", 0, [], accepts_toggle
        self.shots = []

    def hold(self, key, frames):
        self.log.append(key)
        if key == "F12" and self.accepts:
            self.mode = "open" if self.mode == "closed" else "closed"
        self.frame += frames

    def console_mode(self):
        return self.mode

    def wait_frames(self, n):
        self.frame += n

    def shot(self, tag):
        self.shots.append(tag)
        return f"/shots/{tag}.png"

    def console(self):
        return oc.Console(self.hold, self.console_mode, self.wait_frames, self.shot)


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
            rig.console().run("item add,79", "x")
        self.assertEqual(rig.log, ["F12"])  # nothing was typed into a closed console

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
