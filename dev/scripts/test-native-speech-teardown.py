#!/usr/bin/env python3
"""Probe admission waits for startup and rejects ambiguous executable identity."""
from pathlib import Path
import runpy
from types import SimpleNamespace
import tempfile
import unittest
from unittest.mock import patch

MODULE = runpy.run_path(str(Path(__file__).with_name('native-speech-teardown.py')))


class ExecutableAdmission(unittest.TestCase):
    def test_paced_startup_waits_for_the_actual_executable(self):
        replies = [SimpleNamespace(stdout='other /proc/1/exe\n'),
                   SimpleNamespace(stdout='expected /proc/2/exe\n')]
        with patch('subprocess.run', side_effect=replies), patch('time.sleep') as sleep:
            MODULE['wait_for_executable']('probe', 'expected')
        sleep.assert_called_once()

    def test_wrong_executable_cannot_admit_the_run(self):
        with patch('subprocess.run', return_value=SimpleNamespace(stdout='other /proc/1/exe\n')), \
                patch('time.monotonic', side_effect=[0, 46]):
            with self.assertRaisesRegex(ValueError, 'exactly one'):
                MODULE['wait_for_executable']('probe', 'expected')

    def test_existing_receipts_cannot_be_relabelled_as_a_new_run(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            MODULE['validate_output'](output)
            (output / 'phase-0.json').write_text('{}')
            with self.assertRaisesRegex(ValueError, 'prior stage receipts'):
                MODULE['validate_output'](output)

    def test_duplicate_matching_processes_are_ambiguous(self):
        with patch('subprocess.run', return_value=SimpleNamespace(stdout='expected /proc/1/exe\nexpected /proc/2/exe\n')):
            with self.assertRaisesRegex(ValueError, 'exactly one'):
                MODULE['wait_for_executable']('probe', 'expected')


if __name__ == '__main__':
    unittest.main()
