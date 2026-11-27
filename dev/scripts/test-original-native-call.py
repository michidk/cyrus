#!/usr/bin/env python3
"""Semantic native-call observation without executable-specific layouts."""
import struct
import unittest
from original_native_call import read_native_call


class NativeCallTests(unittest.TestCase):
    def setUp(self):
        self.memory = {100: struct.pack('<HB', 1, 0), 200: struct.pack('<3I', 0, 501, 601)}
        self.read = lambda address, size: (self.memory[200][address-200:address-200+size]
                                         if 200 <= address < 212 else self.memory[address][:size])
        self.functions = [{'name': 'NullFunction'}, {'name': 'cameraController'}, {'name': 'Rnd'}]

    def observe(self, **options):
        return read_native_call(self.read, self.functions, call=100, table=200,
                                receiver=300, selected_source=300, handler=501, **options)

    def test_resolves_installed_name_and_keeps_receiver_distinct(self):
        self.assertEqual(self.observe(), dict(function='cameraController', function_id=1,
            argument_count=0, receiver_is_selected_source=True))
        self.assertEqual(read_native_call(self.read, self.functions, call=100, table=200,
            receiver=400, selected_source=300, handler=501)['receiver_is_selected_source'], False)

    def test_stale_handler_cannot_identify_the_call(self):
        with self.assertRaisesRegex(ValueError, 'handler differs'):
            read_native_call(self.read, self.functions, call=100, table=200,
                             receiver=300, selected_source=300, handler=601)

    def test_missing_truncated_and_invalid_bindings_reject(self):
        for field in ('call', 'table', 'receiver', 'selected_source', 'handler'):
            values=dict(call=100, table=200, receiver=300, selected_source=300, handler=501)
            values[field]=None
            with self.subTest(field=field), self.assertRaises(ValueError):
                read_native_call(self.read, self.functions, **values)
        for payload in (b'\x01', struct.pack('<HB',3,0), struct.pack('<HB',0,0), struct.pack('<HB',1,9)):
            self.memory[100]=payload
            with self.subTest(payload=payload), self.assertRaises(ValueError):self.observe()
        self.memory[100]=struct.pack('<HB',1,0);self.memory[200]=b'\0'
        with self.assertRaises(ValueError):self.observe()


if __name__ == '__main__': unittest.main()
