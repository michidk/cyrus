#!/usr/bin/env python3
"""Original locals observation requires an authored count and a complete read."""
import unittest
from original_script_locals import read_locals


class ScriptLocals(unittest.TestCase):
    def test_reads_each_instances_offset_using_the_authored_count(self):
        import struct
        pool = bytes(4) + struct.pack('<4i', 2, -7, 2, 19)
        reads = []
        def read(pointer, size):
            reads.append((pointer, size))
            return pool[pointer - 100:pointer - 100 + size]
        self.assertEqual(read_locals(read, 100, 4, 2, 'first'), [2, -7])
        self.assertEqual(read_locals(read, 100, 12, 2, 'second'), [2, 19])
        self.assertEqual(reads, [(104, 8), (112, 8)])

    def test_missing_count_is_unavailable_not_empty_locals(self):
        with self.assertRaisesRegex(ValueError, 'actor: authored local count unavailable'):
            read_locals(lambda *_: b'', 100, 4, None, 'actor')

    def test_empty_authored_allocation_does_not_read_memory(self):
        self.assertEqual(read_locals(lambda *_: self.fail('unexpected read'), 0, 0, 0, 'empty'), [])

    def test_missing_pool_or_instance_binding_rejects_nonempty_locals(self):
        for pool, offset in [(0, 4), (100, 0), (100, -1), (None, 4)]:
            with self.subTest(pool=pool, offset=offset), self.assertRaisesRegex(ValueError, 'actor: local allocation'):
                read_locals(lambda *_: b'', pool, offset, 2, 'actor')

    def test_truncated_read_and_invalid_count_reject_contextually(self):
        with self.assertRaisesRegex(ValueError, 'actor: expected 8 local bytes, observed 4'):
            read_locals(lambda *_: bytes(4), 100, 4, 2, 'actor')
        for count in [-1, True, 1.5]:
            with self.subTest(count=count), self.assertRaisesRegex(ValueError, 'authored local count'):
                read_locals(lambda *_: b'', 100, 4, count, 'actor')


if __name__ == '__main__':
    unittest.main()
