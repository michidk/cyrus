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

    def test_silent_tail_cannot_steal_a_real_anchor(self):
        rng = np.random.default_rng(10)
        reference = self.reference * 3
        audible = reference * .7 + rng.normal(0, 900, len(reference))
        # A numerically tiny delayed copy can score highly after cancellation
        # of large cumulative energies. It must never displace audible anchors.
        tail = reference * 1e-9
        recording = np.concatenate((self.padding, audible, np.zeros(8000), tail))
        result = module.analyze(recording, reference)
        self.assertTrue(result['success'])
        self.assertAlmostEqual(result['onset_seconds'], .2)
        self.assertTrue(all(a['gain'] > .5 for a in result['anchors']))
        refined = module.normalize(recording, reference)
        self.assertTrue(refined['success'])
        self.assertTrue(all(a['gain'] > .5 for a in refined['anchors']))

    def test_exact_three_hundred_ms_span_uses_sample_units(self):
        recording = np.zeros(len(self.reference))
        recording[3200:6400] = self.reference[3200:6400]
        result = module.analyze(recording, self.reference)
        self.assertTrue(result['success'])
        self.assertEqual(len(result['anchors']), 4)

    def test_fractional_sample_alignment_recovers_gain(self):
        reference = module.resample_poly(module.resample_poly(self.reference, 1, 2), 2, 1)
        fine = module.resample_poly(reference, 16, 1)
        delayed = np.concatenate((np.zeros(1600 * 16 + 5), fine * .65, np.zeros(1600 * 16)))
        recording = module.resample_poly(delayed, 1, 16)
        coarse = module.analyze(recording, reference)
        result = module.normalize(recording, reference)
        self.assertTrue(result['success'])
        self.assertLess(coarse['median_gain'], result['median_gain'] - .01)
        self.assertAlmostEqual(result['median_gain'], .65, delta=.01)
        self.assertAlmostEqual(result['onset_seconds'], .2 + 5 / 128000, delta=1 / 128000)

    def test_overlapping_cues_keep_their_independent_anchors(self):
        other = np.random.default_rng(9).normal(0, 1000, 8000)
        recording = np.zeros(16000)
        recording[1600:9600] += self.reference * .5
        recording[5600:13600] += other * .5
        for reference, onset in [(self.reference, .2), (other, .7)]:
            result = module.normalize(recording, reference)
            self.assertTrue(result['success'])
            self.assertAlmostEqual(result['onset_seconds'], onset)
            self.assertAlmostEqual(result['median_gain'], .5, delta=.03)

    def test_normalized_report_marks_overlapping_continuation(self):
        spec = importlib.util.spec_from_file_location('normalized', Path(__file__).with_name('book-audio-normalize.py'))
        normalized = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(normalized)
        other = np.random.default_rng(9).normal(0, 1000, 8000)
        recording = np.zeros(16000)
        recording[1600:9600] += self.reference * .5
        recording[5600:13600] += other * .5
        result = normalized.report(recording, [self.reference, other], .1)
        self.assertTrue(result['success'])
        self.assertAlmostEqual(result['references'][0]['input_relative_onset_seconds'], .1)
        # Stronger first cue preserves admitted anchors after the second starts.
        recording[1600:9600] += self.reference
        result = normalized.report(recording, [self.reference, other], .1)
        first = result['references'][0]
        self.assertTrue(first['success'])
        self.assertTrue(any(a['overlaps'] == [1] for a in first['anchors']))

    def test_refinement_does_not_admit_a_gap_or_wrong_cue(self):
        gap = np.concatenate((self.padding, self.reference[:4000], np.zeros(400), self.reference[4000:]))
        for recording in (gap, np.zeros(16000), np.random.default_rng(8).normal(0, 1000, 16000)):
            result = module.normalize(recording, self.reference)
            self.assertFalse(result['success'])
            self.assertIsNone(result['median_gain'])

    def test_normalized_cli_preserves_reports_and_rejects_missing_continuation(self):
        import json
        import subprocess
        import sys
        script = Path(__file__).with_name('book-audio-normalize.py')
        with tempfile.TemporaryDirectory() as temp:
            temp = Path(temp)
            reference, monitor, output = (temp / name for name in ('ref.wav', 'monitor.wav', 'report.json'))
            module.wavfile.write(reference, 8000, self.reference.astype(np.int16))
            module.wavfile.write(monitor, 8000, np.concatenate((self.padding, self.reference * .5)).astype(np.int16))
            command = [sys.executable, str(script), str(monitor), str(output),
                       '--reference', str(reference), '--input-seconds', '.1']
            subprocess.run(command, check=True, capture_output=True)
            before = output.read_bytes()
            self.assertTrue(json.loads(before)['success'])
            self.assertEqual(subprocess.run(command, capture_output=True).returncode, 2)
            self.assertEqual(output.read_bytes(), before)
            module.wavfile.write(monitor, 8000, np.concatenate((self.reference[:4000], np.zeros(400), self.reference[4000:])).astype(np.int16))
            command[3] = str(temp / 'rejected.json')
            self.assertEqual(subprocess.run(command, capture_output=True).returncode, 1)
            self.assertFalse(json.loads((temp / 'rejected.json').read_text())['success'])

    def test_silence_and_wrong_sound_are_not_continuity(self):
        for recording in (np.zeros(12000), np.random.default_rng(7).normal(0, 1000, 12000)):
            self.assertFalse(module.analyze(recording, self.reference)['success'])

if __name__ == '__main__':
    unittest.main()
