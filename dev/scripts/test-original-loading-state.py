#!/usr/bin/env python3
"""Loading selection snapshots retain the distinction between policy and old index."""
import struct
import unittest
from original_loading_state import read_loading_state


class LoadingStateTests(unittest.TestCase):
    def setUp(self):
        self.memory = {100: struct.pack('<I', 1), 200: struct.pack('<I', 3)}
        self.bindings = dict(artwork_enabled=100, retained_frame=200)

    def observe(self):
        return read_loading_state(lambda address, size: self.memory[address][:size], self.bindings)

    def test_disabled_artwork_keeps_stale_index_without_claiming_selection(self):
        self.assertEqual(self.observe(), dict(artwork_enabled=True, retained_frame=3))
        self.memory[100] = struct.pack('<I', 0)
        self.assertEqual(self.observe(), dict(artwork_enabled=False, retained_frame=3))

    def test_missing_truncated_and_invalid_policy_reject(self):
        for field in self.bindings.copy():
            address = self.bindings.pop(field)
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, field):
                self.observe()
            self.bindings[field] = address
            self.memory[address] = b'\0'
            with self.subTest(field=field), self.assertRaisesRegex(ValueError, field):
                self.observe()
            self.memory[address] = struct.pack('<I', 0)
        self.memory[100] = struct.pack('<I', 2)
        with self.assertRaisesRegex(ValueError, 'artwork_enabled'):
            self.observe()


if __name__ == '__main__':
    unittest.main()
