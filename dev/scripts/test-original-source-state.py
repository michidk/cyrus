#!/usr/bin/env python3
"""Decoded temporary-source observations require every bound semantic field."""
import struct
import unittest
from original_source_state import read_source_state


class SourceState(unittest.TestCase):
    def setUp(self):
        self.memory = bytearray(2048)
        self.memory[100:356] = bytes(range(256))
        struct.pack_into('<3i', self.memory, 400, 1, -2, 3)
        struct.pack_into('<3i', self.memory, 420, -1, 2049, 0)
        struct.pack_into('<9I', self.memory, 440, *range(9))
        struct.pack_into('<3I', self.memory, 480, 10, 11, 12)
        struct.pack_into('<2i', self.memory, 520, -7, 9)
        struct.pack_into('<2I', self.memory, 608, 700, 710)
        self.memory[700:704] = b'one\0'
        self.memory[710:711] = b'\0'
        self.bindings = dict(attributes=100, position_q8=400, angles=420, matrix=440,
                             model_position=480, local_pool=500, local_offset=20,
                             string_pool=600, string_offset=8)
        self.script = dict(name='ambient', local_count=2, string_count=2)

    def read(self, pointer, size):
        return self.memory[pointer:pointer + size]

    def observe(self):
        return read_source_state(self.read, self.bindings, self.script, {3: 'health'})

    def test_complete_observation_preserves_unsigned_attributes_and_signed_orientation(self):
        value = self.observe()
        self.assertEqual(value['attributes']['named'], {'health': 3})
        self.assertEqual(value['attributes']['unnamed']['255'], 255)
        self.assertEqual(len(value['attributes']['unnamed']), 255)
        self.assertEqual(value['position_q8'], [1, -2, 3])
        self.assertEqual(value['orientation']['angles'], [-1, 2049, 0])
        self.assertEqual(value['orientation']['matrix'], [[0, 1, 2], [3, 4, 5], [6, 7, 8]])
        self.assertEqual(value['orientation']['model_position'], [10, 11, 12])
        self.assertEqual(value['script_context'], dict(name='ambient', variables=[-7, 9], strings=['one', '']))

    def test_current_pools_are_read_instead_of_reusing_preload_values(self):
        before = self.observe()
        struct.pack_into('<2i', self.memory, 820, 5, 6)
        struct.pack_into('<2I', self.memory, 908, 1000, 1004)
        self.memory[1000:1004] = b'two\0'; self.memory[1004:1008] = b'end\0'
        self.bindings.update(local_pool=800, string_pool=900)
        after = self.observe()
        self.assertNotEqual(before['script_context'], after['script_context'])
        self.assertEqual(after['script_context']['variables'], [5, 6])
        self.assertEqual(after['script_context']['strings'], ['two', 'end'])

    def test_truncated_attributes_orientation_and_tables_reject(self):
        for pointer in (100, 440, 608):
            with self.subTest(pointer=pointer), self.assertRaisesRegex(ValueError, 'ambient'):
                read_source_state(lambda p, n: b'' if p == pointer else self.read(p, n),
                                  self.bindings, self.script, {3: 'health'})

    def test_each_last_element_remains_part_of_the_observation(self):
        before = self.observe()
        for pointer in (355, 475, 527, 702):
            with self.subTest(pointer=pointer):
                old = self.memory[pointer]
                self.memory[pointer] ^= 1
                self.assertNotEqual(self.observe(), before)
                self.memory[pointer] = old

    def test_decoded_last_attribute_change_rejects_shared_semantic_comparison(self):
        import importlib.util
        from pathlib import Path
        spec = importlib.util.spec_from_file_location('source_pair', Path(__file__).with_name('compare-segment.py'))
        pair = importlib.util.module_from_spec(spec); spec.loader.exec_module(pair)
        expected = {'native_source': {'kind': 'separate_temporary', 'state': self.observe()}}
        self.memory[355] ^= 1
        measured = {'native_source': {'kind': 'separate_temporary', 'state': self.observe()}}
        failed = [row for row in pair.semantic_delta(expected, measured) if row['status'] != 'match']
        self.assertEqual(len(failed), 1)
        self.assertEqual(failed[0]['field'], 'native_source.state.attributes.unnamed.255')

    def test_zero_authored_counts_are_empty_without_binding_an_unavailable_pool(self):
        self.script.update(local_count=0, string_count=0)
        self.bindings.update(local_pool=0, local_offset=0, string_pool=0, string_offset=0)
        self.assertEqual(self.observe()['script_context'], dict(name='ambient', variables=[], strings=[]))

    def test_missing_metadata_and_invalid_string_reject_instead_of_inventing_empty(self):
        del self.script['local_count']
        with self.assertRaises(ValueError):
            self.observe()
        self.script['local_count'] = 2
        struct.pack_into('<I', self.memory, 608, 0)
        with self.assertRaisesRegex(ValueError, 'string'):
            self.observe()
        with self.assertRaises(ValueError):
            read_source_state(self.read, self.bindings, self.script, {3: 'same', 4: 'same'})


if __name__ == '__main__':
    unittest.main()
