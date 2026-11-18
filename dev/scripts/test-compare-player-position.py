#!/usr/bin/env python3
"""Exercise the equal-clock comparison through its CLI, using synthetic positions."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('compare-player-position.py')


def point(clock, x, y=0, z=0):
    return dict(elapsed_scale=clock, position_q8=[x, y, z])


class Comparison(unittest.TestCase):
    def run_comparison(self, reference, native):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name, series in [('reference', reference), ('native', native)]:
                (root / (name + '.json')).write_text(json.dumps(series))
            return subprocess.run([sys.executable, str(SCRIPT),
                '--reference', str(root / 'reference.json'),
                '--native', str(root / 'native.json')], capture_output=True, text=True)

    def test_interpolates_by_elapsed_clock_instead_of_update_ordinal(self):
        result = self.run_comparison([point(0, 0), point(10, 100), point(30, 140)],
                                     [point(0, 0), point(5, 50), point(20, 120), point(30, 140)])
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report['matching'], 4)
        self.assertEqual(report['results'][2]['reference_bracket'], [10, 30])
        self.assertEqual(report['results'][2]['reference_position_q8'], [120, 0, 0])

    def test_reports_signed_residuals_and_never_extrapolates(self):
        result = self.run_comparison([point(0, 0), point(10, 10, 20, 30)],
                                     [point(5, 6, 8, 18), point(11, 11)])
        self.assertEqual(result.returncode, 1, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report['results'][0]['delta_q8'], [1, -2, 3])
        self.assertEqual(report['outside_reference'], 1)
        self.assertNotIn('reference_position_q8', report['results'][1])

    def test_rejects_ambiguous_or_invalid_clock_series(self):
        for series in [[point(0, 0), point(0, 1)], [point(2, 0), point(1, 0)],
                       [dict(elapsed_scale=0, position_q8=[0, 1])], []]:
            with self.subTest(series=series):
                result = self.run_comparison(series, [point(0, 0)])
                self.assertEqual(result.returncode, 2)
                self.assertIn('reference', result.stderr)


if __name__ == '__main__':
    unittest.main()
