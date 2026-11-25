#!/usr/bin/env python3
"""Negative controls for the public mono source-clock consumer."""
from copy import deepcopy
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('clock', Path(__file__).with_name('native-source-clock.py'))
clock = importlib.util.module_from_spec(spec)
spec.loader.exec_module(clock)


def snapshot():
    return {'samples': {'ownership': {'samples': {'slots': [{'token': 7}]}},
                        'players': [{'token': 7, 'entity': 18, 'paused': False, 'transport': {
                            'source_identity': 'fixture', 'input_samples': 2, 'right_bits': None,
                            'speed': 1.0, 'repetitions': 1}}]}}


class SourceClock(unittest.TestCase):
    def test_pending_right_sample_has_not_been_consumed(self):
        data = snapshot()
        self.assertEqual(clock.source(data, 0)['output_samples'], 4)
        data['samples']['players'][0]['transport']['right_bits'] = 0
        self.assertEqual(clock.source(data, 0)['output_samples'], 3)

    def test_fractional_intervals_accumulate_without_per_interval_rounding(self):
        initial = clock.source(snapshot(), 0)
        current = dict(initial, output_samples=initial['output_samples'] + 1)
        clock.assert_progress(initial, current, 2 * 40_000, 8000)
        with self.assertRaisesRegex(ValueError, 'expected 1.*observed 0'):
            clock.assert_progress(initial, initial, 2 * 40_000, 8000)

    def test_cursor_owner_and_source_changes_are_rejected(self):
        initial = clock.source(snapshot(), 0)
        for field, value in [('token', 8), ('entity', 19), ('source_identity', 'replacement'), ('owner', 'replacement')]:
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, field):
                clock.assert_progress(initial, dict(initial, **{field: value}), 0, 8000)
        with self.assertRaisesRegex(ValueError, 'expected 0.*observed 1'):
            clock.assert_progress(initial, dict(initial, output_samples=5), 0, 8000)

    def test_wrong_sample_rate_does_not_pass_as_fractional_error(self):
        initial = clock.source(snapshot(), 0)
        current = dict(initial, output_samples=initial['output_samples'] + 16)
        clock.assert_progress(initial, current, 1_000_000, 8000)
        with self.assertRaisesRegex(ValueError, 'expected 22.*observed 16'):
            clock.assert_progress(initial, current, 1_000_000, 11025)

    def test_missing_duplicate_and_unsupported_players_are_rejected(self):
        original = snapshot()
        changes = [lambda x: x['samples']['players'].clear(),
                   lambda x: x['samples']['players'].append(deepcopy(x['samples']['players'][0])),
                   lambda x: x['samples']['ownership']['samples']['slots'].__setitem__(0, None),
                   lambda x: x['samples']['players'][0].__setitem__('paused', True),
                   lambda x: x['samples']['players'][0]['transport'].__setitem__('speed', 2.0)]
        for change in changes:
            data = deepcopy(original)
            change(data)
            with self.assertRaises(ValueError):
                clock.source(data, 0)


if __name__ == '__main__':
    unittest.main()
