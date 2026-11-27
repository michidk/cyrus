"""Completed support records preserve miss metadata without inventing contact."""
import struct
import unittest
from original_support_state import read_support_state


class SupportStateTests(unittest.TestCase):
    def setUp(self):
        self.bindings = dict(source_kind=0, classification=1, actor_height=2,
                             normal=6, texture=18, metadata=20)
        self.data = struct.pack('<BBi3iHB', 0x20, 4, -12345, 0, -256, 0, 7, 4)

    def observe(self):
        return read_support_state(lambda offset, size: self.data[offset:offset+size], self.bindings)

    def test_model_support_retains_exact_height_and_metadata(self):
        self.assertEqual(self.observe(), dict(source='model', present=True, classification=4,
            actor_height_q8=-12345, normal=[0,-256,0], texture=7, metadata=4))

    def test_miss_metadata_cannot_be_mistaken_for_support(self):
        self.data = struct.pack('<BBi3iHB', 0, 0, -(1 << 27), 0, -256, 0, 7, 4)
        result = self.observe()
        self.assertFalse(result['present'])
        self.assertEqual((result['normal'], result['texture'], result['metadata']), ([0,-256,0],7,4))

    def test_incomplete_or_contradictory_results_reject(self):
        for kind, classification, height in ((0,4,-12345),(0x20,0,-12345),
                                             (0x40,4,-(1<<27)),(3,4,-12345)):
            self.data = struct.pack('<BBi3iHB',kind,classification,height,0,-256,0,7,4)
            with self.subTest(kind=kind),self.assertRaises(ValueError):self.observe()
        self.data = b'\0'
        with self.assertRaisesRegex(ValueError,'expected'):self.observe()
        self.bindings.pop('source_kind')
        with self.assertRaisesRegex(ValueError,'binding'):self.observe()


if __name__ == '__main__': unittest.main()
