#!/usr/bin/env python3
"""Controls for the original-fight driver's pure logic, independent of the original and of game bytes."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('fight', Path(__file__).with_name('original-fight.py'))
fight = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fight)


def actor(identifier, group, health, selected=False, pending=None, drawn=0):
    return {"id": identifier, "selected": selected, "group": group, "health": health,
            "pending": pending, "drawn": drawn}


def sample(pg=8, rg=8, rh=250, drawn=1, ph=100):
    return {"frame": 100, "fps": 12, "pg": pg, "ph": ph, "pdrawn": drawn, "rg": rg, "rpending": None, "rh": rh}


class DriverLogic(unittest.TestCase):
    def test_view_reads_both_actors_and_tolerates_a_late_opponent(self):
        state = {"counters": {"mainloop_frame": 7, "fps": 4}, "actors": [
            actor(1, 8, 100, selected=True, drawn=1), actor(55, 102, 240, pending=8)]}
        view = fight.view(state)
        self.assertEqual((view["frame"], view["pg"], view["rg"], view["rh"], view["rpending"]), (7, 8, 102, 240, 8))
        state["actors"].pop()
        view = fight.view(state)
        self.assertEqual((view["pg"], view["rg"], view["rh"]), (8, None, None))
        self.assertIsNone(fight.view({"counters": {"mainloop_frame": 1, "fps": 1}, "actors": [actor(55, 8, 250)]}))
        self.assertIsNone(fight.view(None))
        self.assertIsNone(fight.view({"error": "world changing"}))

    def test_a_tap_needs_the_armed_idle_animation_and_a_drawn_sword(self):
        for gate in ("busy", "any"):
            self.assertFalse(fight.ready_to_tap(sample(pg=102, rg=102), gate))
            self.assertFalse(fight.ready_to_tap(sample(pg=0, rg=102), gate))
            # The reader's drawn byte stays 0 in the armed idle group, so it must not gate a tap.
        self.assertTrue(fight.ready_to_tap(sample(drawn=0, rg=102), "busy"))
        self.assertTrue(fight.ready_to_tap(sample(drawn=0, rg=8), "any"))
        self.assertTrue(fight.ready_to_tap(sample(rg=102), "busy"))
        self.assertTrue(fight.ready_to_tap(sample(rg=8), "any"))

    def test_the_busy_gate_waits_while_the_opponent_is_idle_unengaged_or_absent(self):
        for group in (8, 0, None):
            self.assertFalse(fight.ready_to_tap(sample(rg=group), "busy"))
        for group in (91, 92, 98, 102, 117):
            self.assertTrue(fight.ready_to_tap(sample(rg=group), "busy"))

    def test_tap_outcomes_separate_hits_reactions_responses_and_misses(self):
        before = sample(rg=102, rh=250)
        self.assertEqual(fight.classify_tap(before, [sample(rh=250), sample(rh=240)]), {"outcome": "hit", "damage": 10})
        self.assertEqual(fight.classify_tap(before, [sample(rh=250), sample(rh=220)])["damage"], 30)
        self.assertEqual(fight.classify_tap(before, [sample(rg=117, rh=250)])["outcome"], "reaction")
        response = fight.classify_tap(before, [sample(rg=98, rh=250), sample(rg=99, rh=250)])
        self.assertEqual((response["outcome"], response["groups"]), ("response", [98, 99]))
        self.assertEqual(fight.classify_tap(before, [sample(rg=102, rh=250)])["outcome"], "miss")
        self.assertEqual(fight.classify_tap(before, [])["outcome"], "miss")

    def test_summary_counts_taps_by_opponent_state_and_lists_damage_per_hit(self):
        def tap(rg, outcome, damage=0):
            return {"ev": "tap", "before": sample(rg=rg), "outcome": outcome, "damage": damage}
        rows = [tap(102, "hit", 10), tap(102, "miss"), tap(8, "response"), tap(8, "hit", 20),
                {"ev": "end", "rh": 220, "ph": 100, "frames": 900, "reason": "frame-budget"}]
        summary = fight.summarize(rows)
        self.assertEqual((summary["taps"], summary["hits"], summary["damage"]), (4, 2, [10, 20]))
        self.assertEqual(summary["by_state"]["busy"], {"taps": 2, "hit": 1, "reaction": 0, "response": 0, "miss": 1})
        self.assertEqual(summary["by_state"]["idle"], {"taps": 2, "hit": 1, "reaction": 0, "response": 1, "miss": 0})
        self.assertEqual(summary["end"]["reason"], "frame-budget")


class FakeSampler:
    """Yields one scripted (view, raw) pair per get(): frame counter and opponent group."""

    def __init__(self, steps):
        self.steps = list(steps)

    def get(self):
        frame, group = self.steps.pop(0) if len(self.steps) > 1 else self.steps[0]
        raw = {"counters": {"mainloop_frame": frame}}
        return (sample(rg=group) | {"frame": frame}), raw


class FrameWaits(unittest.TestCase):
    def test_a_condition_met_inside_the_frame_budget_returns_that_sample(self):
        sampler = FakeSampler([(10, None), (20, None), (30, 102)])
        result = fight.wait_frames(sampler, 100, 5, until=lambda s: s["rg"] == 102)
        self.assertEqual(result["frame"], 30)

    def test_the_frame_budget_not_wall_time_ends_a_wait_with_a_condition(self):
        sampler = FakeSampler([(10, None), (60, None), (130, None)])
        with self.assertRaisesRegex(TimeoutError, "within 100 frames"):
            fight.wait_frames(sampler, 100, 5, until=lambda s: s["rg"] == 102)

    def test_a_plain_frame_wait_returns_once_the_counter_has_advanced(self):
        sampler = FakeSampler([(10, 8), (14, 8), (21, 8)])
        self.assertEqual(fight.wait_frames(sampler, 10, 5)["frame"], 21)


if __name__ == '__main__':
    unittest.main()
