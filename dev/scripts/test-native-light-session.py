#!/usr/bin/env python3
"""Mutation controls for public material-light publication observations."""
import importlib.util
from pathlib import Path
import unittest
from copy import deepcopy
from types import SimpleNamespace
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('light_session', Path(__file__).with_name('native-light-session.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def observation(generation=1, entity='old'):
    return {'bound_light_image': {'world': 7, 'generation': generation, 'entity': entity,
        'material': 'material', 'image': 'image', 'dimensions': [8, 257],
        'format': 'Rgba8Unorm', 'rows_after_lookup_256': [[1, 2, 3, 4, 5, 6, 7, 8]]}}


class LightSessionControls(unittest.TestCase):
    def test_replacement_requires_retired_owner_and_saved_rows(self):
        before, after = observation(), observation(2, 'replacement')
        module.assert_bound(before, after, True)
        for field, value in [('generation', 1), ('entity', 'old'),
                             ('rows_after_lookup_256', []), ('world', 8)]:
            changed = deepcopy(after)
            changed['bound_light_image'][field] = value
            with self.assertRaises(ValueError):
                module.assert_bound(before, changed, True)

    def test_rejection_preserves_live_bindings_and_actual_rows(self):
        before = observation()
        module.assert_bound(before, deepcopy(before), False)
        for field, value in [('image', 'new'), ('material', 'new'), ('entity', 'new'),
                             ('generation', 2), ('rows_after_lookup_256', [])]:
            changed = deepcopy(before)
            changed['bound_light_image'][field] = value
            with self.assertRaises(ValueError):
                module.assert_bound(before, changed, False)

    def test_logical_light_state_and_owner_mutations_fail(self):
        before = {'world': 1, 'name': 'light', 'map_light': False,
                  'light': {'enabled': True, 'locked': True, 'radius': 200}}
        module.assert_saved_light(before, deepcopy(before))
        for key, value in [('world', 2), ('name', 'other'), ('map_light', True),
                           ('light', {'enabled': False})]:
            after = deepcopy(before)
            after[key] = value
            with self.assertRaises(ValueError):
                module.assert_saved_light(before, after)

    def test_rejected_preparation_releases_public_console(self):
        driver = module.LightDriver(SimpleNamespace(output=Path('.'), prepare_seconds=600))
        requests = []
        driver.request = lambda kind, **fields: requests.append((kind, fields))
        with patch.object(module.capture.ContinuationDriver, 'load', side_effect=RuntimeError('rejected')):
            with self.assertRaisesRegex(RuntimeError, 'rejected'):
                driver.load('control', '/fixture.session')
        self.assertEqual(requests, [('console', {'open': True}), ('console', {'open': False})])

    def test_rejection_acknowledgement_consumes_only_the_expected_error(self):
        driver = module.LightDriver(SimpleNamespace(output=Path('.')))
        def reject(*args, **kwargs):
            raise RuntimeError('map-light eligibility differs from installed placement')
        driver.request = reject
        driver.acknowledge_rejection()
        def unrelated(*args, **kwargs):
            raise RuntimeError('transport failed')
        driver.request = unrelated
        with self.assertRaisesRegex(RuntimeError, 'transport failed'):
            driver.acknowledge_rejection()

    def test_inspection_view_settles_before_material_observation(self):
        driver = module.LightDriver(SimpleNamespace(output=Path('.'), position=[1, 2, 3],
                                                    scene='world', seconds=1, interval=.001))
        states = iter([{'flycam': {'position': [0, 0, 0]}},
                       {'flycam': {'position': [1, 2, 3]}}])
        driver.request = lambda *args, **kwargs: None
        driver.ready = lambda *args: None
        driver.progress = lambda state: next(states)
        driver.observe = lambda: 'positioned material'
        self.assertEqual(driver.view(), 'positioned material')

    def test_empty_fixture_cannot_claim_light_continuation(self):
        empty = observation()
        empty['bound_light_image']['rows_after_lookup_256'] = []
        with self.assertRaisesRegex(ValueError, 'active rendered light'):
            module.assert_bound(empty, empty, False)


if __name__ == '__main__':
    unittest.main()
