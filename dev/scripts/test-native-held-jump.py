#!/usr/bin/env python3
"""Failure controls for the held-jump receipt comparison; no install required."""
import copy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("held_jump", Path(__file__).with_name("native-held-jump.py"))
held = importlib.util.module_from_spec(spec)
spec.loader.exec_module(held)


def receipts():
    observations = [{"frame": 10 + i, "clock_scale": i * 512,
                     "input_state": {"space": 0 < i <= 1},
                     "player": {"selected": True, "yaw": 0, "position_q8": [0, 0, 0], "group": 0,
                                "health": 100, "falling": 0, "queued": 0, "sliding": 0,
                                "living": 0, "rope_attached": False}} for i in range(3)]
    original = {"key": "space", "normal_frame_rate": 12, "observations": observations,
                "steps": [{"frame": 10 + i, "frame_scale": 512,
                           "input_state": {"space": i < 1}} for i in range(2)],
                "input_edges": [dict(copy.deepcopy(observations[1]), input_state={"space": False})],
                "boundaries": [dict(copy.deepcopy(observations[0]), input_state={"space": True}),
                               copy.deepcopy(observations[-1])]}
    boundaries = []
    for i in range(4):
        scale = max(0, i - 1) * 512 + 1024
        if i == 0:
            scale -= 512
        boundaries.append({"after_updates": i, "update": 20 + i, "generation": 1,
                           "normal_frame_rate": 12, "elapsed_scale": scale,
                           "game_seconds": scale / 3072, "key": "Space" if i in (1, 2) else None,
                           "action": "press" if i == 1 else "release" if i == 2 else None,
                           "player": {"actors": [{"source": "player", "name": "CYRUS",
                              "attributes": [0, 0, 0, 100], "animation": {"current_group": 0},
                              "spatial": {"effective": {"position": [0, 0, 0], "angles": [0, 0, 0]}},
                              "control": {"falling_state": 0, "queued_motion": 0, "sliding_state": 0,
                                          "living_state": 0, "rope_attached": False, "stable_position_valid": True}}]}})
    return original, {"boundaries": boundaries}


class Contracts(unittest.TestCase):
    def test_complete_receipts_match(self):
        original, native = receipts()
        self.assertEqual(held.compare(original, native, 2)["matched_boundaries"], 3)

    def test_directional_input_allows_a_displaced_idle_endpoint_only_when_requested(self):
        original, native = receipts()
        original['key'] = 'Up'
        for row in original['observations'] + original['steps'] + original['input_edges'] + original['boundaries']:
            row['input_state'] = {'Up': row['input_state']['space']}
        for row in original['observations'][1:]:
            row['player']['position_q8'][2] = 1024
        # Edges retain the same atomic body as their corresponding observation.
        original['input_edges'][0]['player']['position_q8'][2] = 1024
        original['boundaries'][-1]['player']['position_q8'][2] = 1024
        for i, row in enumerate(native['boundaries']):
            if row['key'] is not None:
                row['key'] = 'ArrowUp'
            if i >= 2:
                row['player']['actors'][0]['spatial']['effective']['position'][2] = 1024
        with self.assertRaisesRegex(ValueError, 'same position'):
            held.compare(original, native, 2, key='ArrowUp', original_key='Up')
        result = held.compare(original, native, 2, key='ArrowUp', original_key='Up',
                              allow_displacement=True)
        self.assertEqual(result['displacement_q8'], [0, 0, 1024])
        self.assertEqual(result['max_position_error_q8'], 0)
        # A different idle animation is not an admitted movement comparison.
        native['boundaries'][0]['player']['actors'][0]['animation']['current_group'] = 2
        with self.assertRaisesRegex(ValueError, 'state diverged'):
            held.compare(original, native, 2, key='ArrowUp', original_key='Up',
                         allow_displacement=True)

    def test_only_released_pre_admission_observations_can_precede_the_press(self):
        original, native = receipts()
        prefix = copy.deepcopy(original['observations'][0])
        prefix['frame'] -= 1
        original['observations'].insert(0, prefix)
        self.assertEqual(held.compare(original, native, 2)['matched_boundaries'], 3)
        prefix['input_state']['space'] = True
        with self.assertRaisesRegex(ValueError, 'pre-admission'):
            held.compare(original, native, 2)

    def test_jump_acceptance_requires_an_observed_queue_fall_and_landing(self):
        original, native = receipts()
        # Identical idle receipts establish equality, never jump admission.
        with self.assertRaisesRegex(ValueError, 'jump lifecycle'):
            held.compare(original, native, 2, require_jump_lifecycle=True)

    def test_jump_lifecycle_reports_complete_cycles_and_rejects_missing_stages(self):
        original, native = receipts()
        template = copy.deepcopy(original['observations'][0])
        original['observations'] = []
        for i in range(5):
            row = copy.deepcopy(template)
            row.update(frame=10 + i, clock_scale=i * 512, input_state={'space': i == 1})
            row['player'].update(queued=int(i == 1), falling=int(i == 2))
            row['player']['position_q8'][1] = -256 if i in (1, 2) else 0
            original['observations'].append(row)
        original['steps'] = [dict(frame=10 + i, frame_scale=512,
                                  input_state={'space': i == 0}) for i in range(4)]
        original['boundaries'] = [dict(copy.deepcopy(original['observations'][0]),
                                       input_state={'space': True}),
                                  copy.deepcopy(original['observations'][-1])]
        original['input_edges'] = [dict(copy.deepcopy(original['observations'][1]),
                                        input_state={'space': False})]
        template = copy.deepcopy(native['boundaries'][0])
        native['boundaries'] = []
        for i in range(6):
            row = copy.deepcopy(template)
            scale = 512 + i * 512
            row.update(after_updates=i, update=20 + i, elapsed_scale=scale,
                       game_seconds=scale / 3072,
                       key='Space' if i in (1, 2) else None,
                       action='press' if i == 1 else 'release' if i == 2 else None)
            player = original['observations'][max(0, i - 1)]['player']
            actor = row['player']['actors'][0]
            actor['spatial']['effective']['position'] = player['position_q8'][:]
            actor['control'].update(queued_motion=player['queued'], falling_state=player['falling'])
            native['boundaries'].append(row)
        result = held.compare(original, native, 4, require_jump_lifecycle=True)
        self.assertEqual(result['jump_lifecycle'],
                         {'queued_at': [1], 'falling_at': [2], 'landed_at': [3]})
        grounded = copy.deepcopy(original)
        for row in grounded['observations'] + grounded['input_edges'] + grounded['boundaries']:
            row['player']['position_q8'][1] = 0
        with self.assertRaisesRegex(ValueError, 'jump lifecycle: no observed airborne lift'):
            held.compare(grounded, native, 4, require_jump_lifecycle=True)
        for field in ('queued', 'falling'):
            damaged = copy.deepcopy(original)
            for row in damaged['observations'] + damaged['input_edges'] + damaged['boundaries']:
                row['player'][field] = 0
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 'jump lifecycle'):
                held.compare(damaged, native, 4, require_jump_lifecycle=True)

    def test_jump_lifecycle_rejects_unfinished_flight(self):
        original, native = receipts()
        for row in original['observations'][1:]:
            row['player']['queued'] = 1
        original['input_edges'][0]['player']['queued'] = 1
        original['boundaries'][-1]['player']['queued'] = 1
        with self.assertRaisesRegex(ValueError, 'jump lifecycle: incomplete'):
            held.compare(original, native, 2, require_jump_lifecycle=True)

    def test_incomplete_native_is_rejected(self):
        original, native = receipts()
        native["boundaries"].pop()
        with self.assertRaisesRegex(ValueError, "incomplete native"):
            held.compare(original, native, 2)

    def test_changed_position_is_rejected(self):
        original, native = receipts()
        native["boundaries"][2]["player"]["actors"][0]["spatial"]["effective"]["position"][1] = 1
        with self.assertRaisesRegex(ValueError, "state diverged"):
            held.compare(original, native, 2)

    def test_changed_boundary_clock_and_input_are_rejected(self):
        for field, value in [("update", 99), ("generation", 2), ("elapsed_scale", 999),
                             ("normal_frame_rate", 6), ("action", "press")]:
            original, native = receipts()
            native["boundaries"][2][field] = value
            with self.subTest(field=field), self.assertRaises(ValueError):
                held.compare(original, native, 2)

    def test_incomplete_original_or_release_is_rejected(self):
        for damage in (lambda o: o["steps"].pop(), lambda o: o["input_edges"].clear(),
                       lambda o: o["observations"][1].update(clock_scale=1)):
            original, native = receipts()
            damage(original)
            with self.assertRaises(ValueError):
                held.compare(original, native, 2)

    def test_public_cli_removes_stale_acceptance_and_retains_failed_raw(self):
        import json
        import subprocess
        import sys
        import tempfile
        original, native = receipts()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, captured, output = (root / name for name in ("original.json", "native.json", "accepted.json"))
            source.write_text(json.dumps(original))
            captured.write_text(json.dumps(native))
            command = [sys.executable, str(Path(held.__file__)), "--original", str(source),
                       "--native", str(captured), "--updates", "2", "--out", str(output)]
            subprocess.run(command, check=True, capture_output=True)
            self.assertEqual(json.loads(output.read_text())["matched_boundaries"], 3)
            idle = subprocess.run(command + ['--require-jump-lifecycle'],
                                  capture_output=True, text=True)
            self.assertNotEqual(idle.returncode, 0)
            self.assertIn('jump lifecycle', idle.stderr)
            self.assertFalse(output.exists())
            self.assertEqual(json.loads(output.with_suffix('.raw.json').read_text()), native)
            native["boundaries"].pop()
            captured.write_text(json.dumps(native))
            failed = subprocess.run(command, capture_output=True, text=True)
            self.assertNotEqual(failed.returncode, 0)
            self.assertIn("incomplete native", failed.stderr)
            self.assertFalse(output.exists())
            self.assertEqual(json.loads(output.with_suffix(".raw.json").read_text()), native)
            original_bytes = source.read_bytes()
            alias = command[:-1] + [str(source)]
            self.assertNotEqual(subprocess.run(alias, capture_output=True).returncode, 0)
            self.assertEqual(source.read_bytes(), original_bytes)

    def test_capture_admits_balanced_edges_and_releases_on_failure(self):
        from unittest.mock import patch
        original, native = receipts()
        for failure in (False, True):
            calls = []
            def call(addr, command):
                calls.append(command)
                if command["type"] == "schedule_input":
                    if failure:
                        raise RuntimeError("deadline expired")
                    return native
            with patch.object(held.trace, "wait_live"), patch.object(held.trace, "call", side_effect=call):
                if failure:
                    with self.assertRaisesRegex(RuntimeError, "deadline"):
                        held.capture("unused", original, 2)
                else:
                    self.assertEqual(held.capture("unused", original, 2), native)
            self.assertEqual(calls[0]["events"], [
                {"after_updates": 0}, {"after_updates": 1, "key": "Space", "action": "press"},
                {"after_updates": 2, "key": "Space", "action": "release"}, {"after_updates": 3}])
            self.assertEqual(calls[-1], {"type": "input", "key": "Space", "action": "release"})

    def test_directional_capture_schedules_and_releases_the_selected_binding(self):
        from unittest.mock import patch
        original, native = receipts()
        original['key'] = 'Up'
        for row in original['observations'] + original['steps'] + original['input_edges'] + original['boundaries']:
            row['input_state'] = {'Up': row['input_state']['space']}
        with patch.object(held.trace, 'wait_live'), patch.object(held.trace, 'call', return_value=native) as call:
            self.assertEqual(held.capture('unused', original, 2, key='ArrowUp', original_key='Up'), native)
        self.assertEqual(call.call_args_list[0].args[1]['events'][1],
                         {'after_updates': 1, 'key': 'ArrowUp', 'action': 'press'})
        self.assertEqual(call.call_args_list[-1].args[1],
                         {'type': 'input', 'key': 'ArrowUp', 'action': 'release'})

    def test_public_cli_accepts_directional_bindings(self):
        import json
        import subprocess
        import sys
        import tempfile
        original, native = receipts()
        original['key'] = 'Up'
        for row in original['observations'] + original['steps'] + original['input_edges'] + original['boundaries']:
            row['input_state'] = {'Up': row['input_state']['space']}
        for row in native['boundaries']:
            if row['key'] is not None:
                row['key'] = 'ArrowUp'
        with tempfile.TemporaryDirectory() as directory:
            source, captured, output = (Path(directory)/name for name in ('o.json', 'n.json', 'out.json'))
            source.write_text(json.dumps(original))
            captured.write_text(json.dumps(native))
            result = subprocess.run([sys.executable, str(Path(held.__file__)),
                '--original', str(source), '--native', str(captured), '--out', str(output),
                '--updates', '2', '--key', 'ArrowUp', '--original-key', 'Up', '--allow-displacement'],
                capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            summary = json.loads(output.read_text())
            self.assertEqual((summary['key'], summary['original_key']), ('ArrowUp', 'Up'))
            self.assertTrue(summary['allow_displacement'])
            self.assertEqual(summary['matched_boundaries'], 3)

    def test_long_capture_rejected_before_taking_input(self):
        original, _ = receipts()
        with self.assertRaisesRegex(ValueError, "2..32"):
            held.capture("unused:1", original, 48)


if __name__ == "__main__":
    unittest.main()
