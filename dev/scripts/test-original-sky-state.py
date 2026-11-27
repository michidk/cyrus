#!/usr/bin/env python3
"""Stopped sky observations preserve lifetime evidence without executable layouts."""
import struct
import unittest
from original_sky_state import read_sky_state


class SkyStateTests(unittest.TestCase):
    def setUp(self):
        formats = dict(enabled=('<I', 1), direction=('<I', 2047), speed=('<f', -0.5),
                       offset_u=('<f', 255.5), offset_v=('<f', -256), last_tick=('<I', 42),
                       texture_handle=('<i', 7))
        self.bindings = {name: 100 + i * 4 for i, name in enumerate(formats)}
        self.memory = {self.bindings[name]: struct.pack(fmt, value)
                       for name, (fmt, value) in formats.items()}

    def observe(self):
        return read_sky_state(lambda address, size: self.memory[address][:size], self.bindings)

    def test_preserves_observed_direction_phase_and_signed_speed(self):
        self.assertEqual(self.observe(), dict(enabled=True, direction=2047, speed=-0.5,
            offset_u=255.5, offset_v=-256.0, last_tick=42, texture_handle=7))
        self.memory[self.bindings['direction']] = struct.pack('<I', 0)
        self.assertEqual(self.observe()['direction'], 0)

    def test_reused_texture_does_not_hide_direction_or_scroll_reset(self):
        before = self.observe()
        for field, payload in [('direction', struct.pack('<I', 17)),
                               ('offset_u', struct.pack('<f', 0)),
                               ('offset_v', struct.pack('<f', 0)),
                               ('last_tick', struct.pack('<I', 0))]:
            self.memory[self.bindings[field]] = payload
        after = self.observe()
        self.assertEqual(before['texture_handle'], after['texture_handle'])
        self.assertNotEqual(before['direction'], after['direction'])
        self.assertEqual((after['offset_u'], after['offset_v'], after['last_tick']), (0, 0, 0))

    def test_missing_and_truncated_fields_cannot_be_filled(self):
        for field in self.bindings.copy():
            address = self.bindings.pop(field)
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, field):
                self.observe()
            self.bindings[field] = address
            original = self.memory[address]
            self.memory[address] = original[:3]
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, field):
                self.observe()
            self.memory[address] = original

    def test_invalid_direction_enable_and_nonfinite_scroll_reject(self):
        for field, payload in [('direction', struct.pack('<I', 2048)),
                               ('enabled', struct.pack('<I', 2)),
                               ('speed', struct.pack('<f', float('nan'))),
                               ('offset_u', struct.pack('<f', float('inf')))]:
            original = self.memory[self.bindings[field]]
            self.memory[self.bindings[field]] = payload
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, field):
                self.observe()
            self.memory[self.bindings[field]] = original


if __name__ == '__main__':
    unittest.main()
