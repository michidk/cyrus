#!/usr/bin/env python3
"""Exercise screenshot filter admission with synthetic pixels, without game data."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

from PIL import Image

spec = importlib.util.spec_from_file_location('filter_check', Path(__file__).with_name('check-book-filter.py'))
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


class StoredFilterGrid(unittest.TestCase):
    def test_accepts_stored_half_weight_at_each_dither_phase(self):
        # floor(255/2)=127, then the documented RGB565 storage at these phases.
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'capture.png'
            image = Image.new('RGB', (2, 2))
            image.putdata([(123, 125, 123), (123, 125, 123),
                           (132, 130, 132), (123, 125, 123)])
            image.save(path)
            check.sample_grid(path, (0, 0, 2, 2), framebuffer=True)

    def test_rejects_continuous_filter_channel_between_stored_steps(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'capture.png'
            Image.new('RGB', (1, 1), (115, 113, 115)).save(path)
            with self.assertRaisesRegex(ValueError, 'four-bit encoded filtering'):
                check.sample_grid(path, (0, 0, 1, 1), framebuffer=True)

    def test_rejects_a_noninterpolated_black_capture(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'capture.png'
            Image.new('RGB', (2, 2)).save(path)
            with self.assertRaises(ValueError):
                check.sample_grid(path, (0, 0, 2, 2), framebuffer=True)


if __name__ == '__main__':
    unittest.main()
