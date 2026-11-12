#!/usr/bin/env python3
"""Clock binding must report observed bounds, never interpolate slow wall time."""
import importlib.util
from pathlib import Path
import unittest
spec = importlib.util.spec_from_file_location('clock', Path(__file__).with_name('book-audio-clock.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class Clock(unittest.TestCase):
    def test_slow_rendering_uses_observed_game_bounds(self):
        samples = [dict(pcm_seconds=0, game_seconds=2), dict(pcm_seconds=4, game_seconds=2.1), dict(pcm_seconds=8, game_seconds=2.4)]
        self.assertEqual(module.bind(samples, 5), [2.1, 2.4])
        self.assertEqual(module.bind(samples, 4), [2.1, 2.1])

    def test_rejects_missing_reset_duplicate_and_unbracketed_clocks(self):
        for samples, onset in [([], 1), ([dict(pcm_seconds=0, game_seconds=2)],1),
            ([dict(pcm_seconds=0, game_seconds=2),dict(pcm_seconds=1,game_seconds=1)],.5),
            ([dict(pcm_seconds=0, game_seconds=2),dict(pcm_seconds=0,game_seconds=3)],0)]:
            with self.assertRaises(ValueError): module.bind(samples, onset)

    def test_capture_binding_uses_pcm_start_and_rate(self):
        spec = importlib.util.spec_from_file_location('normalized', Path(__file__).with_name('book-audio-normalize.py'))
        normalized = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(normalized)
        value = {'references': [{'success': True, 'onset_seconds': .5}]}
        normalized.bind_clock(value, {'start': 100, 'rate': 100,
            'transition': {'clocks': [{'frame': 100, 'game_seconds': 2}, {'frame': 200, 'game_seconds': 2.25}]}})
        self.assertEqual(value['references'][0]['game_clock_bounds_seconds'], [2, 2.25])

if __name__ == '__main__': unittest.main()
