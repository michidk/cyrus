#!/usr/bin/env python3
"""Literal monitor controls reject stale output, missing output and incomplete capture."""
import array
from pathlib import Path
import runpy
import unittest

MODULE = runpy.run_path(str(Path(__file__).with_name('native-ambient-lifecycle.py')))

class MonitorControls(unittest.TestCase):
    def test_active_and_retired_windows(self):
        levels = MODULE['pcm_levels'](array.array('h', [1000, 500] * 22050).tobytes())
        self.assertEqual(levels, [1000, 500])
        MODULE['assert_signal'](levels, True)
        MODULE['assert_signal']([0, 0], False)
        with self.assertRaisesRegex(ValueError, 'presence'):
            MODULE['assert_signal'](levels, False)
        with self.assertRaisesRegex(ValueError, 'presence'):
            MODULE['assert_signal']([0, 0], True)
        with self.assertRaisesRegex(ValueError, 'incomplete'):
            MODULE['pcm_levels'](b'\0' * 100)

if __name__ == '__main__':
    unittest.main()
