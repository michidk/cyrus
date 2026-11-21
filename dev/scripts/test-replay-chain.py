#!/usr/bin/env python3
"""Checkpoint contracts with synthetic public observations, without an install."""
import copy
import importlib.util
import json
import math
import os
import subprocess
import tempfile
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

    def test_pending_fixtures_are_valid_but_outside_the_chain(self):
        root = Path(__file__).resolve().parents[1] / "fixtures"
        for path in sorted((root / "pending").glob("*.replay.json")):
            with self.subTest(path=path):
                replay.validate(json.loads(path.read_text()))

    def test_blocked_ship_approach_waypoint_runs(self):
        # Walking onto that approach is refused by the steep-surface check at the probe
        # (preservation engine/player.md, "Forward slope surface selection"); the waypoint
        # past it must be reached with the run gait.
        path = Path(__file__).resolve().parents[1] / "fixtures" / "chapters" / "001-harbour-dock-partial.replay.json"
        steers = [step["steer"] for step in json.loads(path.read_text())["steps"] if "steer" in step]
        blocked = [steer for steer in steers if steer["position_q8"][0] == 9349120]
        self.assertEqual(len(blocked), 1)
        self.assertEqual(blocked[0]["gait"], "run")

    def test_default_discovery_runs_segments_in_separate_sessions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            scripts = root / "dev/scripts"
            scripts.mkdir(parents=True)
            scripts.joinpath("replay-chain.sh").write_text(Path(__file__).with_name("replay-chain.sh").read_text())
            scripts.joinpath("toolchain.sh").write_text("")
            for folder, name in [("chapters", "000-opening"), ("segments", "bell-monocle")]:
                directory = root / "dev/fixtures" / folder
                directory.mkdir(parents=True)
                directory.joinpath(name + ".replay.json").write_text("{}")
            commands = root / "bin"
            commands.mkdir()
            commands.joinpath("python3").write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$CALLS"\n')
            commands.joinpath("python3").chmod(0o755)
            install = root / "install"
            install.mkdir()
            calls = root / "calls"
            env = dict(os.environ, PATH=f"{commands}:/usr/bin:/bin", REDGUARD_DIR=str(install),
                       CYRUS_REPLAY_BINARY="/unused/cyrus", CYRUS_CONFIG_DIR=str(root / "config"), CALLS=str(calls))
            subprocess.run(["bash", str(scripts / "replay-chain.sh")], env=env, check=True)
            sessions = calls.read_text().splitlines()
            self.assertEqual(len(sessions), 2)
            self.assertIn("config/chapters", sessions[0])
            self.assertIn("chapters/000-opening.replay.json", sessions[0])
            self.assertIn("config/segments/bell-monocle", sessions[1])
            self.assertNotIn("000-opening", sessions[1])

    def test_health_checkpoint_rejects_corrupted_expectation(self):
        self.state["hud"] = {"health": 100}
        self.expected["health"] = 100
        replay.checkpoint(self.state, self.scripts, self.expected)
        self.expected["health"] = 99
        with self.assertRaisesRegex(AssertionError, "health"):
            replay.checkpoint(self.state, self.scripts, self.expected)
        self.expected["health"] = 100
        self.expected["inventory"]["46"] = 1
        self.state["script_hud"]["inventory"]["counts"]["46"] = 1
        replay.checkpoint(self.state, self.scripts, self.expected)
        self.expected["inventory"]["46"] = 0
        with self.assertRaisesRegex(AssertionError, "inventory/46"):
            replay.checkpoint(self.state, self.scripts, self.expected)

    def test_facing_uses_rope_controls_and_balances_keys(self):
        class Control:
            heading = 1000
            calls = []

            def ready(self, world):
                return {"hud": {"heading": self.heading}}

            def request(self, command):
                self.calls.append(command)
                self.heading = 1024

        control = Control()
        replay.face_heading(control, {"world": 20, "heading": 1024, "rope": True, "limit": 2})
        events = control.calls[0]["events"]
        self.assertEqual([e["key"] for e in events[:2]], ["ShiftLeft", "ArrowRight"])
        self.assertEqual([e["action"] for e in events[:4]], ["press", "press", "release", "release"])

    def test_ground_facing_converges_from_a_far_heading_within_the_limit(self):
        class Control:
            heading = 450

            def ready(self, world):
                return {"hud": {"heading": self.heading}}

            def request(self, command):
                held = next(e["after_updates"] for e in command["events"] if e["action"] == "release")
                sign = 1 if command["events"][0]["key"] == "ArrowLeft" else -1
                self.heading = (self.heading + sign * 5 * held) % 2048

        control = Control()
        replay.face_heading(control, {"world": 20, "heading": 24, "rope": False, "limit": 30})
        self.assertLessEqual(abs(control.heading - 24), 3)

    def test_topic_waits_then_selects_displayed_text_with_balanced_keys(self):
        class Control:
            menu_current = 0
            idle = 0
            calls = []

            def ready(self, world):
                if world != 20:
                    raise AssertionError("wrong world")

            def request(self, command):
                self.calls.append(command)
                if command["type"] == "scripts":
                    return {"globals": {"menu_state": int(self.idle > 0), "menu": {
                        "current": self.menu_current, "count": 2,
                        "slots": [{"text": "BELL"}, {"text": "BROKEN BELL"}]}}}
                events = command["events"]
                if "key" not in events[0]:
                    self.idle += 1
                else:
                    if events[0]["key"] == "ArrowDown":
                        self.menu_current += 1
                    if events[0]["action"] != "press" or events[1]["action"] != "release":
                        raise AssertionError("unbalanced menu input")
                return {}

        control = Control()
        replay.choose_topic(control, {"world": 20, "text": "BROKEN BELL", "limit": 5})
        schedules = [c for c in control.calls if c["type"] == "schedule_input"]
        self.assertEqual([c["events"][0].get("key") for c in schedules], [None, "ArrowDown", "ControlLeft"])

    def test_staged_entry_refuses_arbitrary_scene_or_extra_commands(self):
        fixture = {"version": 1, "step_micros": 100000,
                   "entry_scene": "BELLTOWR", "steps": [{"checkpoint": self.expected}]}
        replay.validate(fixture)
        fixture["entry_scene"] = "invented"
        with self.assertRaisesRegex(AssertionError, "staged entry"):
            replay.validate(fixture)

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

    def test_entry_wait_continues_past_76_updates_without_releasing_lockout(self):
        class Control:
            update = 0

            def ready(self, world, **kwargs):
                return {"clock": {"game_seconds": self.update * 2},
                        "scripts": {"ordinary_updates": self.update},
                        "hud": {"lockout": int(self.update < 100)}}

            def request(self, command):
                self.assert_command(command)
                self.update += 1

            def assert_command(self, command):
                if command != {"type": "schedule_input", "events": [
                        {"after_updates": 0}, {"after_updates": 1}]}:
                    raise AssertionError("entry wait must send no keys or state mutations")

        control = Control()
        result = replay.await_control(control, {
            "world": 1, "max_seconds": 600, "max_updates": 1000})
        self.assertEqual(control.update, 100)
        self.assertEqual(result["hud"]["lockout"], 0)

    def test_wait_requires_attachment_then_scripted_inventory_completion(self):
        class Control:
            update = 0

            def ready(self, world, **kwargs):
                return {"clock": {"game_seconds": self.update},
                        "scripts": {"ordinary_updates": self.update},
                        "hud": {"lockout": 0},
                        "script_hud": {"inventory": {"counts": {"46": int(self.update >= 8)}}}}

            def request(self, command):
                if command["type"] == "scripts":
                    return {"actors": [{"source": "player", "control": {
                        "rope_attached": self.update >= 3}}]}
                self.update += 1

        control = Control()
        wait = {"world": 20, "max_seconds": 20, "max_updates": 20,
                "rope_attached": True}
        replay.await_control(control, wait)
        self.assertEqual(control.update, 3)
        replay.await_control(control, dict(wait, inventory={"46": 1}))
        self.assertEqual(control.update, 8)
        with self.assertRaisesRegex(AssertionError, "bound"):
            replay.await_control(control, dict(wait, inventory={"46": 2}))

    def test_entry_wait_reports_clock_bound_without_clearing_lockout(self):
        class Control:
            update = 0

            def ready(self, world, **kwargs):
                return {"clock": {"game_seconds": self.update * 2},
                        "scripts": {"ordinary_updates": self.update},
                        "hud": {"lockout": 1}}

            def request(self, command):
                self.update += 1

        with self.assertRaisesRegex(AssertionError, "clock bound"):
            replay.await_control(Control(), {"world": 1, "max_seconds": 3, "max_updates": 100})

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

    def test_turns_hold_the_key_for_at_least_two_updates(self):
        # The game ignores a one-update turn, so a heading error of 48-127 never closed.
        class Control:
            heading = 1960
            holds = []

            def ready(self):
                return {"scripts": {"world": 1}, "hud": {"health": 100, "heading": self.heading}}

            def response(self, command):
                return {"ok": True, "result": self.request(command)}

            def request(self, command):
                if command["type"] == "scripts":
                    return {"world": 1, "player_position_q8": [0, 0, 0]}
                events = command["events"]
                if any(e.get("key") == "ArrowLeft" for e in events):
                    self.holds.append(next(e["after_updates"] for e in events if e.get("action") == "release"))
                    self.heading = (self.heading + 100) % 2048
                return {}

        # Target bearing 0 (+Z) from heading 1960: an error of 88 turns left.
        with self.assertRaises(AssertionError):
            replay.steer(Control(), {"world": 1, "position_q8": [0, 0, 100000], "tolerance": 10, "limit": 1})
        self.assertGreaterEqual(min(Control.holds), 2)

    def test_segment_steering_corrects_coast_before_accepting_stop(self):
        class Control:
            position = [0, 0, 0]
            coasting = True
            schedules = []

            def ready(self, world=None):
                heading = 1024 if self.position[2] > 1000 else 0
                return {"scripts": {"world": 20}, "hud": {"health": 100, "heading": heading}}

            def request(self, command):
                if command["type"] == "scripts":
                    return {"world": 20, "player_position_q8": list(self.position)}
                self.schedules.append(command["events"])
                if self.coasting:
                    self.position[2] += 500
                    self.coasting = False
                return {}

            def response(self, command):
                self.schedules.append(command["events"])
                self.position[2] += -500 if self.position[2] > 1000 else 500
                return {"ok": True}

        control = Control()
        control.position = [0, 0, 1000]
        observed = replay.steer(control, {"world": 20, "position_q8": [0, 0, 1000],
                                         "tolerance": 1, "limit": 3,
                                         "forward_updates": 1, "settle_updates": 2, "stop_updates": 50})
        self.assertEqual(observed["scripts"]["player_position_q8"], [0, 0, 1000])
        self.assertEqual(control.position, [0, 0, 1000])
        self.assertEqual(len(control.schedules), 3)

    def test_final_approach_walks_and_waits_for_the_stop(self):
        class Control:
            position = [0, 0, 0]
            events = None

            def ready(self, world=None):
                return {"scripts": {"world": 20}, "hud": {"health": 100, "heading": 0}}

            def request(self, command):
                if command["type"] == "scripts":
                    return {"world": 20, "player_position_q8": list(self.position)}
                return {}

            def response(self, command):
                self.events = command["events"]
                self.position[2] = 1000
                return {"ok": True}

        control = Control()
        replay.steer(control, {"world": 20, "position_q8": [0, 0, 1000],
                              "tolerance": 1, "limit": 2, "gait": "run",
                              "approach_radius": 2000, "stop_updates": 50})
        self.assertEqual([e.get("key") for e in control.events[:2]], ["ShiftLeft", "ArrowUp"])
        self.assertEqual(control.events[-1]["after_updates"], 51)
        self.assertEqual(control.position, [0, 0, 1000])

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

    def test_session_steps_validate_and_reject_bad_names(self):
        fixture = {"version": 1, "step_micros": 40000, "entry_scene": "EXTPALAC", "resume_session": "entry-a",
                   "steps": [{"setup": {"cheat": {"drevil": 1}, "console": ["item add,79"], "place_player": {"x": 1, "y": 2, "z": 3, "heading": 4}}},
                             {"capture_session": "fight-a"},
                             {"fight": {"world": 30, "opponent": "RICHTONP", "target_health": 220, "max_seconds": 20}},
                             {"await_ending": {"world": 30, "max_updates": 100, "max_seconds": 60}},
                             {"checkpoint": self.expected}]}
        replay.validate(fixture)
        for mutate in (lambda f: f.update(resume_session="../x"),
                       lambda f: f["steps"][1].update(capture_session="A b"),
                       lambda f: f["steps"][2]["fight"].update(max_seconds=0),
                       lambda f: f["steps"][0]["setup"].update(unknown=1),
                       lambda f: f["steps"][3]["await_ending"].update(max_updates=0),
                       lambda f: f.pop("entry_scene")):
            broken = copy.deepcopy(fixture)
            mutate(broken)
            with self.assertRaises(AssertionError):
                replay.validate(broken)

    def test_session_capture_writes_the_document_and_resume_requires_it(self):
        import tempfile

        class Control:
            def __init__(self):
                self.calls = []

            def request(self, command):
                self.calls.append(command["type"])
                if command["type"] == "session_capture_result":
                    return {"pending": False, "document": "CYRUS-SESSION 1\nabc"}
                if command["type"] == "session_load_result":
                    return {"pending": False, "committed": True}
                return {}

        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "sessions" / "entry.session"
            control = Control()
            replay.capture_session(control, path)
            self.assertEqual(path.read_text(), "CYRUS-SESSION 1\nabc")
            replay.resume_session(control, path)
            self.assertIn("session_load", control.calls)
            with self.assertRaisesRegex(AssertionError, "missing resume session"):
                replay.resume_session(control, Path(root) / "absent.session")

    def test_await_ending_needs_a_living_player_then_the_book(self):
        class Control:
            def __init__(self, health, leave_at):
                self.health, self.leave_at, self.updates, self.polls = health, leave_at, 0, 0

            def request(self, command):
                if command["type"] == "schedule_input":
                    self.updates += 1
                    return {}
                self.polls += 1
                if self.updates >= self.leave_at:
                    return {"state": "Menu" if self.polls % 2 == 0 else "Loading", "scripts": None}
                return {"state": "InGame", "scripts": {"failed": 0}, "hud": {"health": self.health}}

        wait = {"world": 30, "max_updates": 20, "max_seconds": 5}
        replay.await_ending(Control(100, 3), wait)
        with self.assertRaisesRegex(AssertionError, "player died"):
            replay.await_ending(Control(0, 3), wait)
        with self.assertRaisesRegex(AssertionError, "never started"):
            replay.await_ending(Control(100, 99), wait)

    def fight_control(self, losses):
        class Control:
            def __init__(self):
                self.health = 250
                self.losses = list(losses)
                self.updates = 0

            def request(self, command):
                kind = command["type"]
                if kind == "schedule_input":
                    self.updates += 1
                    if self.updates % 4 == 0 and self.losses:
                        self.health -= self.losses.pop(0)
                    return {}
                if kind == "scripts":
                    actors = [{"name": "OTHER", "attributes": [0, 0, 0, 1]}]
                    if self.health > 0:
                        actors.append({"name": "RICHTONP", "attributes": [0, 0, 0, self.health]})
                    start = command.get("offset", 0)
                    return {"actors": actors[start:start + command.get("limit", 64)], "total": len(actors)}
                return {"scripts": {"failed": 0}, "hud": {"health": 100}, "clock": {"game_seconds": self.updates * 0.04}}

            def ready(self, world):
                return self.request({"type": "state"})

        return Control()

    def test_fight_accepts_whole_hits_until_the_target_health(self):
        replay.fight(self.fight_control([20, 10, 10]), {"world": 30, "opponent": "RICHTONP", "target_health": 210, "max_seconds": 60})

    def test_fight_to_zero_accepts_a_defeated_opponent_leaving_the_list(self):
        replay.fight(self.fight_control([100, 100, 50]), {"world": 30, "opponent": "RICHTONP", "target_health": 0, "max_seconds": 60})
        with self.assertRaisesRegex(AssertionError, "vanished"):
            replay.fight(self.fight_control([250]), {"world": 30, "opponent": "RICHTONP", "target_health": 100, "max_seconds": 60})

    def test_fight_rejects_partial_hits_and_a_missed_target(self):
        with self.assertRaisesRegex(AssertionError, "not whole hits"):
            replay.fight(self.fight_control([15]), {"world": 30, "opponent": "RICHTONP", "target_health": 200, "max_seconds": 60})
        with self.assertRaisesRegex(AssertionError, "fight clock bound"):
            replay.fight(self.fight_control([]), {"world": 30, "opponent": "RICHTONP", "target_health": 200, "max_seconds": 1})


if __name__ == "__main__":
    unittest.main()
