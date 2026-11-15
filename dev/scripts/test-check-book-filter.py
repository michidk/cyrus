#!/usr/bin/env python3
"""Exercise the book-filter comparison CLI with passing and wrong-domain output."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from PIL import Image

SCRIPT = Path(__file__).with_name('check-book-filter.py')


class BookFilter(unittest.TestCase):
    def test_encoded_complements_pass_and_linear_light_midpoints_fail(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            first, second = root / 'a.png', root / 'b.png'
            for values, accepted in [((127, 128), True), ((188, 188), False)]:
                Image.new('RGB', (2, 2), (values[0],) * 3).save(first)
                Image.new('RGB', (2, 2), (values[1],) * 3).save(second)
                run = subprocess.run([sys.executable, str(SCRIPT), 'compare', str(first),
                                      str(second), '--rectangle', '0', '0', '2', '2'],
                                     capture_output=True, text=True, check=False)
                self.assertEqual(run.returncode == 0, accepted, run.stderr)
                result = json.loads(run.stdout)
                self.assertEqual(result['maximum_complement_error'],
                                 abs(sum(values) - 255))

    def test_fixture_retains_dimensions_and_transparency(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, target = root / 'source.menu', root / 'target.menu'
            source.write_text(json.dumps({'textures': [{'width': 2, 'height': 1,
                                                       'rgba': [9, 8, 7, 0, 6, 5, 4, 255]}]}))
            subprocess.run([sys.executable, str(SCRIPT), 'prepare', str(source), str(target),
                            '--inverse'], check=True)
            self.assertEqual(json.loads(target.read_text())['textures'],
                             [{'width': 2, 'height': 1, 'rgba': [255, 255, 255, 0, 0, 0, 0, 255]}])


if __name__ == '__main__':
    unittest.main()
