#!/usr/bin/env python3
"""Arrival selection and dock-frame regression through public session receipts."""
from copy import deepcopy
import unittest
import json
from pathlib import Path
import subprocess
import sys
import tempfile

from native_session_assertions import assert_arrival


def fixture():
    actors = []
    bindings = []
    observations = []
    for index, name in enumerate(('boat', 'dock')):
        animation = ['MODEL', 4] if index else None
        actors.append({'name': name, 'control': {'hidden': False, 'invisible': False},
                       'animation_failure': None, 'update_failure': None,
                       'runtime': {'state': {'failure': None}},
                       'model': {'current': {'animation': animation}}})
        bindings.append({'actor': [1, index], 'visibility': 0, 'displayed': animation})
        pose = {'model': 'MODEL', 'frame': 4} if index else None
        observations.append({'rendered_actor': {'world': 1, 'name': name,
            'actor': {'generation': 2, 'index': index},
            'logical_pose': {'animation': pose}, 'displayed_pose': pose,
            'model': {'model': 'MODEL', 'frame': 4, 'visibility': 0} if index else None}})
    before = {'logical': {'registry': {'current': 1, 'worlds': [{
        'world': 1, 'generation': 1, 'content_id': 'same', 'source_identity': 'same',
        'actors': actors, 'actor_lists': {'traversal_prepared': True,
                                       'selected': [[1, 0], [1, 1]]}}]}},
        'entity_bindings': bindings}
    after = deepcopy(before)
    world = after['logical']['registry']['worlds'][0]
    world['generation'] = 2
    world['actor_lists']['selected'] = [[2, 0], [2, 1]]
    for binding in after['entity_bindings']:
        binding['actor'][0] = 2
    expected = {'world': 1, 'actors': [
        {'index': 0, 'name': 'boat', 'animated': False},
        {'index': 1, 'name': 'dock', 'animated': True}]}
    return before, after, observations, expected


class Arrival(unittest.TestCase):
    def test_static_boat_and_settled_dock_frame_survive_restore(self):
        self.assertEqual(assert_arrival(*fixture())['actors_checked'], 2)

    def test_frames_may_advance_after_the_capture(self):
        values = fixture()
        actor = values[2][1]['rendered_actor']
        actor['displayed_pose']['frame'] = 5
        actor['model']['frame'] = 5
        assert_arrival(*values)

    def test_cli_rejects_corrupted_selection(self):
        values = fixture()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            paths = [root / name for name in ('before', 'after', 'expected', 'observation0', 'observation1')]
            before, after, observations, expected = values
            for path, value in zip(paths, (before, after, expected, *observations)):
                text = json.dumps(value)
                if path.name in ('before', 'after'):
                    value['version'] = 1
                    text = 'CYRUS-SESSION 1\n' + '0' * 64 + '\n' + json.dumps(value)
                path.write_text(text)
            command = [sys.executable, '-B', str(Path(__file__).with_name('check-arrival-session.py')),
                       *map(str, paths)]
            passed = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(passed.returncode, 0, passed.stderr)
            after['logical']['registry']['worlds'][0]['actor_lists']['selected'].pop(0)
            paths[1].write_text('CYRUS-SESSION 1\n' + '0' * 64 + '\n' + json.dumps(after))
            failed = subprocess.run(command, capture_output=True, text=True)
            self.assertEqual(failed.returncode, 1)
            self.assertIn('boat: absent from cached draw selection', failed.stderr)

    def test_missing_cached_boat_selection_fails_before_or_after_restore(self):
        for side in (0, 1):
            values = fixture()
            values[side]['logical']['registry']['worlds'][0]['actor_lists']['selected'].pop(0)
            with self.subTest(side=side), self.assertRaisesRegex(ValueError, 'boat.*selection'):
                assert_arrival(*values)

    def test_hidden_owner_and_stale_generation_are_not_visible(self):
        for field, value in (('visibility', 1), ('actor', [7, 0])):
            values = fixture()
            values[1]['entity_bindings'][0][field] = value
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, 'boat.*owner'):
                assert_arrival(*values)

    def test_logical_hidden_actor_and_script_failure_reject_acceptance(self):
        for mutate in (lambda a: a['control'].update(hidden=True),
                       lambda a: a['runtime']['state'].update(failure='failed')):
            values = fixture()
            mutate(values[1]['logical']['registry']['worlds'][0]['actors'][0])
            with self.assertRaises(ValueError):
                assert_arrival(*values)

    def test_bind_pose_stale_mesh_and_missing_observation_fail(self):
        for key, value in (('displayed_pose', {'model': 'MODEL', 'frame': 0}),
                           ('model', {'model': 'MODEL', 'frame': 0, 'visibility': 0}),
                           ('model', None)):
            values = fixture()
            values[2][1]['rendered_actor'][key] = value
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, 'dock.*frame'):
                assert_arrival(*values)
        values = fixture()
        values[2].pop()
        with self.assertRaisesRegex(ValueError, 'dock.*observation'):
            assert_arrival(*values)

    def test_unchanged_generation_or_changed_sources_do_not_establish_restore(self):
        for key, value in (('generation', 1), ('content_id', 'different')):
            values = fixture()
            values[1]['logical']['registry']['worlds'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                assert_arrival(*values)

    def test_invalid_expectations_reject_negative_indices_and_ambiguous_kinds(self):
        for key, value in (('index', -1), ('animated', 'false'), ('name', '')):
            values = fixture()
            values[3]['actors'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                assert_arrival(*values)

    def test_duplicate_receipt_or_binding_is_rejected(self):
        for part in ('observation', 'binding'):
            values = fixture()
            if part == 'observation':
                values[2].append(deepcopy(values[2][0]))
            else:
                values[1]['entity_bindings'].append(deepcopy(values[1]['entity_bindings'][0]))
            with self.subTest(part=part), self.assertRaises(ValueError):
                assert_arrival(*values)


if __name__ == '__main__':
    unittest.main()
