#!/usr/bin/env python3
"""Controls for original reader region ownership, independent of game bytes."""
import importlib.util
import io
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('region', Path(__file__).with_name('original-memory-region.py'))
region = importlib.util.module_from_spec(spec)
spec.loader.exec_module(region)


class MemoryRegion(unittest.TestCase):
    def test_unmerged_and_coalesced_mapping_bind_the_same_region(self):
        memory = io.BytesIO(bytes(23) + b'anchor' + bytes(35))
        self.assertEqual(region.locate(memory, [(20, 60)], b'anchor', 3, 40), 20)
        self.assertEqual(region.locate(memory, [(0, 64)], b'anchor', 3, 40), 20)

    def test_partial_region_and_duplicate_signature_are_rejected(self):
        for blob, ranges in [(bytes(50) + b'anchor' + bytes(8), [(0, 64)]),
                             (bytes(3) + b'anchor' + bytes(20) + b'anchor' + bytes(35), [(0, 70)])]:
            with self.subTest(ranges=ranges), self.assertRaisesRegex(ValueError, 'expected one binding'):
                region.locate(io.BytesIO(blob), ranges, b'anchor', 3, 40)

    def test_absent_signature_never_falls_back_to_mapping_start(self):
        with self.assertRaisesRegex(ValueError, 'found 0'):
            region.locate(io.BytesIO(bytes(80)), [(0, 80)], b'anchor', 3, 40)


if __name__ == '__main__':
    unittest.main()
