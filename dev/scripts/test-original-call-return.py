#!/usr/bin/env python3
"""Direct-call witnesses reject stale stack data and wrong call targets."""
import struct
import unittest
from original_call_return import DirectCallReturn


class ReturnTests(unittest.TestCase):
    def setUp(self):
        self.memory = {100: b'\xe8' + struct.pack('<i', 200-105),
                       300: struct.pack('<I', 105)}

    def witness(self, **changes):
        return DirectCallReturn(lambda address, size: self.memory[address][:size],
                                **dict(dict(call=100, target=200, return_slot=300), **changes))

    def test_direct_target_and_actual_read_are_both_required(self):
        witness = self.witness()
        with self.assertRaisesRegex(ValueError, 'not been observed'): witness.result()
        self.assertEqual(witness.observe('read'),
                         dict(direct_call_target_matches=True, return_slot_read=True))
        self.assertEqual(witness.result()['return_slot_read'], True)
        with self.assertRaises(ValueError): witness.observe('read')

    def test_wrong_target_and_noncall_or_truncated_code_reject(self):
        with self.assertRaises(ValueError): self.witness(target=201)
        for code in (b'\xe8', b'\x90'*5):
            self.memory[100] = code
            with self.subTest(code=code), self.assertRaises(ValueError): self.witness()

    def test_stale_slot_and_store_cannot_establish_return(self):
        witness = self.witness()
        with self.assertRaises(ValueError): witness.observe('write')
        with self.assertRaises(ValueError): witness.observe('read')
        witness = self.witness()
        self.memory[300] = struct.pack('<I', 106)
        with self.assertRaisesRegex(ValueError, 'overwritten'): witness.observe('read')
        self.memory[300] = struct.pack('<I', 105)
        with self.assertRaises(ValueError): witness.observe('read')
        self.memory[300] = struct.pack('<I', 106)
        with self.assertRaises(ValueError): self.witness()
        self.memory[300] = b'\0'
        with self.assertRaises(ValueError): self.witness()

    def test_missing_bindings_reject(self):
        for field in ('call', 'target', 'return_slot'):
            for value in (None, 0, True):
                with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                    self.witness(**{field:value})


if __name__ == '__main__': unittest.main()
