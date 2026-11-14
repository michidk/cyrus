#!/usr/bin/env python3
"""Admission controls for the SDL presentation producer, independent of game layouts."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch
import tempfile
import json

spec = importlib.util.spec_from_file_location('capture', Path(__file__).with_name('sdl-frame-capture.py'))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


class Capture(unittest.TestCase):
    def test_progress_uses_presented_playback_clock_and_persists_endpoint(self):
        self.run_progress([10, 10, 11, 13], expected=13)

    def test_frozen_gameplay_clock_cannot_admit_movie_progress(self):
        self.run_progress([10, 10, 10, 10], error=TimeoutError)

    def test_reversed_clock_and_missing_generation_reject_without_receipt(self):
        self.run_progress([10, 9], error=ValueError)
        self.run_progress([10, 13], error=ValueError, missing_pcm=True)

    def test_clock_progress_with_frozen_pcm_cannot_complete(self):
        self.run_progress([10, 13], error=TimeoutError, pcm_frames=[1000, 1000])

    def test_requested_pcm_span_outlives_early_clock_endpoint(self):
        self.run_progress([10, 13, 14, 15], expected=15,
                          pcm_frames=[1000, 1100, 1200, 1300], minimum_pcm_frames=300)
        self.run_progress([10, 13], error=TimeoutError,
                          pcm_frames=[1000, 1100], minimum_pcm_frames=300)

    def test_invalid_pcm_span_rejects_before_creating_output(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'series'
            for count in (0, -1, True, 1.5):
                with self.assertRaisesRegex(ValueError, 'minimum PCM frames'):
                    capture.capture_progress('unused', output, {'movie': {'address': 16, 'size': 4}},
                                             'movie', 3, require_pcm=True, minimum_pcm_frames=count)
                self.assertFalse(output.exists())
            with self.assertRaisesRegex(ValueError, 'minimum PCM frames'):
                capture.capture_progress('unused', output, {'movie': {'address': 16, 'size': 4}},
                                         'movie', 3, minimum_pcm_frames=300)

    def run_progress(self, clocks, expected=None, error=None, missing_pcm=False,
                     pcm_frames=None, minimum_pcm_frames=None):
        def sample(directory, output, rows, timeout):
            index = len(list(output.parent.glob('[0-9]*')))
            output.mkdir()
            result = dict(ok=True, owner='sdl_surface_present', sequence=index + 1,
                          monotonic_ns=100 + index, fields={'movie': clocks[index].to_bytes(4, 'little').hex()})
            if not missing_pcm:
                frames = pcm_frames[index] if pcm_frames is not None else 1000 + index
                generation = index if pcm_frames is None else pcm_frames.index(frames)
                result['pcm_generation'] = dict(frames=frames, sequence=generation + 1,
                                                monotonic_ns=90 + generation, rate=48000)
            (output / 'result.json').write_text(json.dumps(result))
            return result
        with tempfile.TemporaryDirectory() as temporary, patch.object(capture, 'capture', side_effect=sample):
            output = Path(temporary) / 'series'
            args = ('unused', output, {'movie': {'address': 16, 'size': 4}}, 'movie', 3)
            options = {} if minimum_pcm_frames is None else dict(minimum_pcm_frames=minimum_pcm_frames)
            if error:
                with self.assertRaises(error):
                    capture.capture_progress(*args, max_captures=len(clocks), require_pcm=True, **options)
                self.assertFalse((output / 'complete.json').exists())
                self.assertTrue((output / '0/result.json').exists())
            else:
                result = capture.capture_progress(*args, max_captures=len(clocks), require_pcm=True, **options)
                self.assertEqual(result['last'], expected)
                expected_span = (pcm_frames[-1] - pcm_frames[0]) if pcm_frames else len(clocks) - 1
                self.assertEqual(result['pcm_frames'], expected_span)
                self.assertEqual(result['pcm_rate'], 48000)
                self.assertEqual(result['minimum_pcm_frames'], minimum_pcm_frames or 1)
                self.assertEqual(json.loads((output / 'complete.json').read_text()), result)
                self.assertEqual(len(json.loads((output / 'progress.json').read_text())), len(clocks))

    def test_bindings_are_bounded_and_not_paths_or_json(self):
        self.assertEqual(capture.bindings({'clock': {'address': 16, 'size': 4}})['clock']['size'], 4)
        for bad in ({}, {'x': {'address': True, 'size': 4}}, {'x': {'address': 1, 'size': 4097}},
                    {'x"': {'address': 1, 'size': 4}}, {'x': {'address': -1, 'size': 4}}):
            with self.assertRaises(ValueError):
                capture.bindings(bad)

    def test_indirect_bindings_follow_only_prior_frozen_fields(self):
        rows = {'owner': {'address': 16, 'size': 8},
                'state': {'pointer': {'field': 'owner', 'offset': 0, 'width': 8, 'addend': 0}, 'size': 4}}
        self.assertIs(capture.bindings(rows), rows)
        for pointer in ({'field': 'missing', 'offset': 0, 'width': 8, 'addend': 0},
                        {'field': 'state', 'offset': 0, 'width': 8, 'addend': 0},
                        {'field': 'owner', 'offset': 1, 'width': 8, 'addend': 0},
                        {'field': 'owner', 'offset': 0, 'width': 3, 'addend': 0},
                        {'field': 'owner', 'offset': 0, 'width': True, 'addend': 0},
                        {'field': 'owner', 'offset': 0, 'width': 8, 'addend': 2**63}):
            with self.subTest(pointer=pointer), self.assertRaisesRegex(ValueError, 'pointer'):
                capture.bindings(dict(owner=rows['owner'], state=dict(pointer=pointer, size=4)))

    def test_unbounded_deadline_is_rejected_before_request_creation(self):
        for value in (0, -1, float('inf'), float('nan'), True):
            with self.assertRaisesRegex(ValueError, 'deadline'):
                capture.capture('unused', 'unused', {}, timeout=value)

    def test_scheduled_clock_requires_a_bound_u32_before_creating_output(self):
        rows = {'clock': {'address': 16, 'size': 4}}
        for bad in ({'field': 'absent', 'value': 1}, {'field': 'clock', 'value': True},
                    {'field': 'clock', 'value': -1}, {'field': 'clock', 'value': 2**32}):
            with self.assertRaisesRegex(ValueError, 'scheduled capture'):
                capture.capture('unused', 'unused', rows, at_clock=bad)

    def test_polling_or_truncated_binding_cannot_claim_presentation(self):
        expected = {'clock': {'address': 16, 'size': 4}}
        good = dict(ok=True, owner='sdl_surface_present', sequence=7, thread=42,
                    monotonic_ns=100, fields={'clock': '01000000'})
        self.assertIs(capture.frozen_result(good, expected), good)
        for bad in ([], dict(good, owner='poll'), dict(good, ok=False), dict(good, sequence=True),
                    dict(good, fields={'clock': '01'}), dict(good, fields={})):
            with self.assertRaises(ValueError):
                capture.frozen_result(bad, expected)


if __name__ == '__main__':
    unittest.main()
