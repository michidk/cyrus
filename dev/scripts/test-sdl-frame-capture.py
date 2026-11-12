#!/usr/bin/env python3
"""Admission controls for the SDL presentation producer, independent of game layouts."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('capture', Path(__file__).with_name('sdl-frame-capture.py'))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


class Capture(unittest.TestCase):
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
