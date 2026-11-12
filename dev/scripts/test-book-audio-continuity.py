#!/usr/bin/env python3
"""Synthetic sample-timeline controls; no installed data is included."""
import importlib.util
from pathlib import Path
import unittest
import tempfile
import numpy as np

spec = importlib.util.spec_from_file_location('continuity', Path(__file__).with_name('book-audio-continuity.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class Continuity(unittest.TestCase):
    def setUp(self):
        self.reference = np.random.default_rng(42).normal(0, 1000, 8000)
        self.padding = np.zeros(1600)

    def test_delayed_scaled_reference_has_shared_onset_and_gain(self):
        result = module.analyze(np.concatenate((self.padding, self.reference * .4, self.padding)), self.reference)
        self.assertTrue(result['success'])
        self.assertAlmostEqual(result['onset_seconds'], .2)
        self.assertAlmostEqual(result['median_gain'], .4)

    def test_inserted_underrun_fails_despite_matching_fragments(self):
        recording = np.concatenate((self.padding, self.reference[:4000], np.zeros(400), self.reference[4000:], self.padding))
        result = module.analyze(recording, self.reference)
        self.assertFalse(result['success'])
        self.assertGreater(result['delay_spread_seconds'], .049)
        self.assertIsNone(result['median_gain'])
        self.assertIsNone(result['onset_seconds'])

    def test_missing_samples_fail(self):
        result = module.analyze(np.concatenate((self.padding, self.reference[:4000], self.reference[4400:], self.padding)), self.reference)
        self.assertFalse(result['success'])

    def test_wav_sample_rates_and_rejected_sample_layout(self):
        with tempfile.TemporaryDirectory() as temp:
            reference = Path(temp) / 'reference.wav'
            monitor = Path(temp) / 'monitor.wav'
            module.wavfile.write(reference, 8000, self.reference.astype(np.int16))
            samples = module.resample_poly(np.concatenate((self.padding, self.reference, self.padding)), 6, 1)
            module.wavfile.write(monitor, 48000, np.column_stack((samples, samples)).astype(np.int16))
            self.assertTrue(module.analyze(module.load(monitor), module.load(reference))['success'])
            module.wavfile.write(reference, 8000, np.zeros(100, dtype=np.uint8))
            with self.assertRaisesRegex(ValueError, 'signed 16-bit PCM'):
                module.load(reference)

    def test_silence_and_wrong_sound_are_not_continuity(self):
        for recording in (np.zeros(12000), np.random.default_rng(7).normal(0, 1000, 12000)):
            self.assertFalse(module.analyze(recording, self.reference)['success'])

if __name__ == '__main__':
    unittest.main()
