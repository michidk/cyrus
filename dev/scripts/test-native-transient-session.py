#!/usr/bin/env python3
"""Mutation controls for actual transient geometry and retirement assertions."""
from copy import deepcopy
import importlib.util
from pathlib import Path
import unittest
from types import SimpleNamespace
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('transients', Path(__file__).with_name('native-transient-session.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def observation(generation=1):
    return {'geometry': {'world': 7, 'generation': generation, 'camera_rotation': [0, 0, 0, 1], 'entities': [f'entity-{generation}'],
        'retained': [{'slot': 0}], 'logical': [{'slot': 0}], 'slots': [{
            'admitted': True, 'vertices': [[-1, 1, 0], [1, 1, 0], [1, -1, 0], [-1, -1, 0]],
            'translation': [1, 2, 3], 'rotation': [0, 0, 0, 1], 'texture_hash': 'pixel-hash', 'alpha': 255, 'visibility': 'Some(Visible)', 'texture': 'texture',
            'mesh': f'mesh-{generation}', 'material': f'material-{generation}'}]}}


class TransientControls(unittest.TestCase):
    def test_first_update_rejects_missing_geometry_and_stale_owner(self):
        before, after = observation(), observation(2)
        module.assert_geometry(before, after, True)
        for key, value in [('admitted', False), ('vertices', [[0, 0, 0]] * 4),
                           ('texture', None), ('translation', [9, 2, 3]), ('mesh', 'mesh-1'),
                           ('material', 'material-1'), ('visibility', 'Hidden'), ('alpha', 0),
                           ('rotation', [1, 0, 0, 0]), ('texture_hash', 'wrong-pixels')]:
            changed = deepcopy(after)
            changed['geometry']['slots'][0][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                module.assert_geometry(before, changed, True)
        for key, value in [('generation', 1), ('entities', ['entity-1']), ('world', 8),
                           ('retained', []), ('logical', [])]:
            changed = deepcopy(after)
            changed['geometry'][key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                module.assert_geometry(before, changed, True)

    def test_rejection_preserves_every_observed_binding(self):
        before = observation()
        module.assert_geometry(before, deepcopy(before), False)
        changed = deepcopy(before)
        changed['geometry']['slots'][0]['texture'] = 'new'
        with self.assertRaises(ValueError):
            module.assert_geometry(before, changed, False)

    def test_idle_placeholder_rotation_is_not_submitted_output(self):
        before = observation()
        unused = deepcopy(before['geometry']['slots'][0])
        unused.update(slot=1, admitted=False, rotation=[0, 0, 0, 1])
        before['geometry']['slots'].append(unused)
        after = deepcopy(before)
        after['geometry']['slots'][1]['rotation'] = [.2, 0, 0, .98]
        module.assert_geometry(before, after, False)
        after['geometry']['slots'][1]['mesh'] = 'different owner'
        with self.assertRaises(ValueError):
            module.assert_geometry(before, after, False)
        after = deepcopy(before)
        after['geometry']['camera_rotation'] = [.2, 0, 0, .98]
        after['geometry']['slots'][0]['rotation'] = [.2, 0, 0, .98]
        with self.assertRaises(ValueError):
            module.assert_geometry(before, after, False)

    def test_rejected_preparation_samples_live_geometry_and_releases_console(self):
        driver = module.TransientDriver(SimpleNamespace(output=Path('.'), prepare_seconds=1))
        driver.phase = lambda name: None
        requests = []
        def request(kind, **fields):
            requests.append((kind, fields))
            if kind == 'session_load_result':
                raise RuntimeError('saved environmental effect source changed')
            return {}
        driver.request = request
        driver.observe = lambda: observation()
        rejection, samples = driver.reject('/invalid.session', observation())
        self.assertIn('source changed', rejection)
        self.assertEqual(len(samples), 1)
        self.assertEqual(requests[-1], ('console', {'open': False}))
        changed = observation()
        changed['geometry']['slots'][0]['admitted'] = False
        driver.observe = lambda: changed
        with self.assertRaisesRegex(ValueError, 'not published'):
            driver.reject('/invalid.session', observation())
        self.assertEqual(requests[-1], ('console', {'open': False}))

    def test_capture_waits_only_for_explicit_unready_boundaries(self):
        driver = module.TransientDriver(SimpleNamespace(output=Path('.'), seconds=1, interval=0))
        document = 'CYRUS-SESSION 1\n' + '0' * 64 + '\n{"version":1}'
        errors = [RuntimeError('session_capture_result: player model 123v0: instance is not ready'),
                  RuntimeError('session_capture_result: dialogue [111, 115]: export still in flight at save boundary')]
        driver.request = lambda *args, **kwargs: {}
        with patch.object(driver, 'terminal', side_effect=errors + [{'document': document}]):
            self.assertEqual(driver.capture(), document.encode())
        with patch.object(driver, 'terminal', side_effect=RuntimeError('invalid binding')):
            with self.assertRaisesRegex(RuntimeError, 'invalid binding'):
                driver.capture()

    def test_diagnostic_position_uses_captured_camera_pose(self):
        snapshot = {'camera': {'pose': {'translation': [1, 2, -3], 'rotation': [0, 0, 0, 1]}}}
        self.assertEqual(module.view_position(snapshot), [-20, -40, 65276])
        # A changed actual view must move the diagnostic point; requested input is not evidence.
        snapshot['camera']['pose']['translation'][0] = 7
        self.assertEqual(module.view_position(snapshot)[0], -140)

    def test_fixture_can_use_installed_catalog_at_captured_view_position(self):
        snapshot = {'version': 1, 'logical': {'registry': {'current': 7,
            'worlds': [{'world': 7, 'effects': {'slots': [None, None]}}]}},
            'render_playback': {'textures': {'images': [{'image': 'installed'}]}}}
        data = b'CYRUS-SESSION 1\n' + b'0' * 64 + b'\n' + __import__('json').dumps(snapshot).encode()
        value, image = module.fixture(data, {'images': [
            {'descriptor': 12, 'name': 'installed', 'frames': 4}]}, [100, -200, 65500])
        effect = module.current(value)['effects']['slots'][0]
        self.assertEqual(effect, {'descriptor': 12, 'position': [100, -200, 65500], 'frame': 0, 'speed': 0})
        self.assertEqual(value['render_playback']['textures']['images'][0]['held'], True)
        self.assertEqual(image['frames'], 4)

    def test_fixture_requires_actual_installed_producer(self):
        snapshot = {'version': 1, 'logical': {'registry': {'current': 7,
            'worlds': [{'world': 7, 'effects': {'slots': [None]}}]}}}
        with self.assertRaisesRegex(ValueError, 'installed producer'):
            module.fixture(b'CYRUS-SESSION 1\n' + b'0' * 64 + b'\n' + __import__('json').dumps(snapshot).encode(),
                           {'images': []})


if __name__ == '__main__':
    unittest.main()
