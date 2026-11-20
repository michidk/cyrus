#!/usr/bin/env python3
"""Checkpoint contracts with synthetic public observations, without an install."""
import copy
import importlib.util
import json
import math
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("replay_chain", Path(__file__).with_name("replay-chain.py"))
replay = importlib.util.module_from_spec(spec)
spec.loader.exec_module(replay)


class Checkpoints(unittest.TestCase):
    def setUp(self):
        self.state = {"scripts": {"failed": 0},
                      "script_hud": {"inventory": {"counts": {"1": 2}}}}
        self.scripts = {
            "world": 7,
            "flags": {"flags": [{"definition": {"name": "test_flag"}, "value": 3, "latched": True}]},
            "globals": {"lockout": 0},
            "player_position_q8": [10, 20, 30],
        }
        self.expected = {
            "world": 7, "flags": {"test_flag": {"value": 3, "latched": True}},
            "globals": {"lockout": 0}, "inventory": {"1": 2},
            "position_q8": {"value": [10, 20, 33], "tolerance": 3},
        }

    def test_repository_fixtures_are_input_only_and_share_timing(self):
        fixtures = sorted((Path(__file__).resolve().parents[1] / "fixtures" / "chapters").glob("*.replay.json"))
        self.assertTrue(fixtures, "the replay chain must contain a chapter")
        timings = set()
        for path in fixtures:
            with self.subTest(path=path):
                fixture = json.loads(path.read_text())
                replay.validate(fixture)
                timings.add(fixture["step_micros"])
        self.assertEqual(len(timings), 1)

    def test_modal_input_retries_an_ignored_first_press_and_releases(self):
        class Control:
            def __init__(self):
                self.frame = 0
                self.presses = 0
                self.held = False

            def ready(self, *args, **kwargs):
                return {"scripts": {"ordinary_updates": 233}}

            def request(self, command):
                if command["type"] == "input":
                    self.held = command["action"] == "press"
                    self.presses += self.held
                    return {}
                self.frame += 1
                return {"frame": self.frame, "examine": {"open": self.presses < 2},
                        "scripts": {"failed": 0}}

        control = Control()
        replay.play_modal(control, {"world": 1, "key": "Escape"})
        self.assertEqual(control.presses, 2)
        self.assertFalse(control.held)

    def test_checkpoint_rejects_failed_scripts_even_when_expectations_match(self):
        self.state["scripts"]["failed"] = 1
        with self.assertRaisesRegex(AssertionError, "scripts/failed"):
            replay.checkpoint(self.state, self.scripts, self.expected)

    def test_position_tolerance_includes_boundary(self):
        self.assertEqual(replay.checkpoint(self.state, self.scripts, self.expected), 3)

    def test_corrupted_flag_fails_with_name_and_values(self):
        expected = copy.deepcopy(self.expected)
        expected["flags"]["test_flag"]["value"] = 4
        with self.assertRaisesRegex(AssertionError, "flags/test_flag/value: expected 4, got 3"):
            replay.checkpoint(self.state, self.scripts, expected)

    def test_fixture_rejects_mutating_commands_and_incomplete_checkpoints(self):
        valid = {"version": 1, "step_micros": 100000, "steps": [{"checkpoint": self.expected}]}
        replay.validate(valid)
        for step in [{"set_flag": 1}, {"checkpoint": {"world": 7}},
                     {"checkpoint": self.expected, "input": []}]:
            with self.subTest(step=step), self.assertRaises(AssertionError):
                replay.validate({**valid, "steps": [step]})

    def test_repeat_is_bounded_when_expected_flag_never_arrives(self):
        class Control:
            schedules = 0

            def ready(self, **kwargs):
                return {"hud": {"health": 100}, "scripts": {"world": 7}}

            def request(self, command):
                return {"flags": {"flags": [{"definition": {"name": "flag"}, "value": 0, "latched": False}]}}

            def response(self, command):
                self.schedules += 1
                return {"ok": True}

        control = Control()
        with self.assertRaisesRegex(AssertionError, "input repetition limit exhausted"):
            replay.repeat_inputs(control, {
                "until": {"flags": {"flag": {"value": 1}}}, "limit": 2,
                "steps": [{"input": [{"after_updates": 0}]}],
            })
        self.assertEqual(control.schedules, 2)

    def test_healing_condition_does_not_send_input_above_threshold(self):
        class Control:
            def ready(self):
                return {"hud": {"health": 80}}

            def response(self, command):
                raise AssertionError("unexpected input")

        self.assertFalse(replay.play_inputs(Control(), {
            "when_health_below": 45, "input": [{"after_updates": 0}],
        }))

    def test_steering_converges_from_different_positions_using_only_keys(self):
        class Control:
            def __init__(self, position):
                self.position = list(position)
                self.heading = 800
                self.commands = []

            def ready(self):
                return {"scripts": {"world": 1}, "hud": {"health": 100, "heading": self.heading}}

            def response(self, command):
                return {"ok": True, "result": self.request(command)}

            def request(self, command):
                if command["type"] == "scripts":
                    return {"world": 1, "player_position_q8": self.position}
                self.commands.append(command)
                assert command["type"] == "schedule_input"
                keys = {e.get("key") for e in command["events"] if e.get("action") == "press"}
                held = next(e["after_updates"] for e in command["events"] if e.get("action") == "release")
                assert 1 <= held <= 4
                advanced = max(e["after_updates"] for e in command["events"])
                if "ArrowLeft" in keys:
                    self.heading = (self.heading + 32 * held) % 2048
                elif "ArrowRight" in keys:
                    self.heading = (self.heading - 32 * held) % 2048
                else:
                    assert keys == {"ShiftLeft", "ArrowUp"}
                    self.position[0] -= math.sin(self.heading * math.tau / 2048) * 500 * advanced
                    self.position[2] += math.cos(self.heading * math.tau / 2048) * 500 * advanced
                return {}

        for start in [[0, 0, 0], [2500, 0, -700]]:
            control = Control(start)
            replay.steer(control, {"world": 1, "position_q8": [4000, 0, 8000], "tolerance": 600, "limit": 100})
            self.assertLessEqual(math.dist(control.position, [4000, 0, 8000]), 600)
            self.assertTrue(control.commands)

    def test_running_waypoint_uses_forward_without_walk_modifier(self):
        class Control:
            position = [0, 0, 0]

            def ready(self):
                return {"scripts": {"world": 1}, "hud": {"health": 100, "heading": 0}}

            def request(self, command):
                return {"world": 1, "player_position_q8": list(self.position)}

            def response(self, command):
                keys = {e.get("key") for e in command["events"] if e.get("action") == "press"}
                if keys != {"ArrowUp"}:
                    raise AssertionError(f"running waypoint pressed {keys}")
                self.position[2] += 500
                return {"ok": True}

        observation = replay.steer(Control(), {
            "world": 1, "position_q8": [0, 0, 2000], "tolerance": 600,
            "limit": 10, "gait": "run",
        })
        self.assertEqual(observation["scripts"]["player_position_q8"], [0, 0, 1500])

    def test_waypoint_returns_trigger_observation_before_later_motion(self):
        outer = self

        class Control:
            reads = 0

            def ready(self):
                return {**outer.state, "scripts": {"world": 7, "failed": 0}, "hud": {"health": 100, "heading": 0}}

            def request(self, command):
                self.reads += 1
                observation = copy.deepcopy(outer.scripts)
                if self.reads > 1:
                    observation["player_position_q8"] = [5000, 20, 30]
                return observation

        control = Control()
        observed = replay.steer(control, {"world": 7, "position_q8": [10, 20, 33], "tolerance": 3, "limit": 1})
        self.assertEqual(replay.checkpoint(observed["state"], observed["scripts"], self.expected), 3)
        with self.assertRaisesRegex(AssertionError, "position_q8"):
            replay.checkpoint(control.ready(), control.request({"type": "scripts"}), self.expected)

    def test_steering_accepts_declared_transition_and_rejects_other_worlds(self):
        class Control:
            def ready(self):
                return {"scripts": {"world": 15}, "hud": {"health": 100}}

        target = {"world": 1, "position_q8": [0, 0, 0], "tolerance": 600, "limit": 3}
        with self.assertRaisesRegex(AssertionError, "unexpected world"):
            replay.steer(Control(), target)
        replay.steer(Control(), {**target, "transition_to": 15})

    def test_steering_stops_at_bound_when_blocked(self):
        class Control:
            count = 0

            def ready(self):
                return {"scripts": {"world": 1}, "hud": {"health": 100, "heading": 0}}

            def response(self, command):
                return {"ok": True, "result": self.request(command)}

            def request(self, command):
                if command["type"] == "scripts":
                    return {"world": 1, "player_position_q8": [0, 0, 0]}
                self.count += 1
                return {}

        control = Control()
        with self.assertRaisesRegex(AssertionError, "steering limit exhausted"):
            replay.steer(control, {"world": 1, "position_q8": [0, 0, 8000], "tolerance": 600, "limit": 3})
        self.assertEqual(control.count, 3)

    def test_each_checkpoint_category_is_checked(self):
        for category, wrong in [("world", 8), ("globals", {"lockout": 1}),
                                ("inventory", {"1": 0}),
                                ("position_q8", {"value": [100, 20, 30], "tolerance": 3})]:
            with self.subTest(category=category):
                expected = copy.deepcopy(self.expected)
                expected[category] = wrong
                with self.assertRaises(AssertionError):
                    replay.checkpoint(self.state, self.scripts, expected)


if __name__ == "__main__":
    unittest.main()
