#!/usr/bin/env python3
"""Exercise journal preparation and consumed-key admission without game data."""
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('reference', Path(__file__).with_name('original-logbook-reference.py'))
reference = importlib.util.module_from_spec(spec)
spec.loader.exec_module(reference)


def state(frame=10, *, suspended=False, shown=False, key=0):
    return {'frame': frame, 'suspended': int(suspended), 'log_list': 42 if shown else 0,
            'log_binding': 38, 'keys': {'38': key}}


class ReferenceTests(unittest.TestCase):
    def run_reference(self, values):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        run = Path(temporary.name)
        (run / 'Redguard').mkdir()
        calls = []
        readings = iter(values)

        def execute(command, **kwargs):
            arguments = command[5:]
            calls.append(arguments)
            if arguments == ['python3', '/reader.py']:
                return subprocess.CompletedProcess(command, 0, json.dumps(next(readings)))
            if arguments == ['xdotool', 'key', 'ctrl+F4']:
                self.assertEqual((run / 'Redguard/LOGBOOK.TXT').read_bytes(),
                                 b'7,RASTER REFERENCE\r\n')
            return subprocess.CompletedProcess(command, 0, '')

        with patch.object(reference.subprocess, 'run', side_effect=execute):
            result = reference.capture('owned', run, '/reader.py', ['capture', '/config/reader.png'],
                                       interval=0)
        return result, calls

    def test_consumed_opening_press_is_released_before_capture(self):
        admitted = state(11, suspended=True, shown=True)
        result, calls = self.run_reference([state(), state(11), state(11),
                                           state(12, key=1), admitted, admitted, admitted])
        self.assertTrue(result['success'])
        self.assertEqual(calls.count(['xdotool', 'keydown', 'l']), 1)
        self.assertLess(calls.index(['xdotool', 'keyup', 'l']),
                        calls.index(['capture', '/config/reader.png']))
        self.assertLess(calls.index(['xdotool', 'key', 'ctrl+F4']),
                        calls.index(['xdotool', 'keydown', 'l']))

    def test_startup_suspension_is_waited_out_before_clock_admission(self):
        admitted = state(11, suspended=True, shown=True)
        result, _ = self.run_reference([state(0, suspended=True), state(), state(11),
                                        state(11), admitted, admitted, admitted])
        self.assertTrue(result['success'])

    def test_capture_is_rejected_when_gameplay_resumes(self):
        admitted = state(11, suspended=True, shown=True)
        with self.assertRaisesRegex(ValueError, 'reader changed during capture'):
            self.run_reference([state(), state(11), state(11), admitted, admitted, state(12)])

    def test_key_release_runs_when_reader_fails_after_press(self):
        calls = []
        with tempfile.TemporaryDirectory() as directory:
            run = Path(directory)
            (run / 'Redguard').mkdir()
            values = iter([state(), state(11), state(11)])

            def execute(command, **kwargs):
                arguments = command[5:]
                calls.append(arguments)
                if arguments == ['python3', '/reader.py']:
                    try:
                        return subprocess.CompletedProcess(command, 0, json.dumps(next(values)))
                    except StopIteration:
                        raise subprocess.CalledProcessError(1, command, stderr='reader unavailable')
                return subprocess.CompletedProcess(command, 0, '')

            with patch.object(reference.subprocess, 'run', side_effect=execute):
                with self.assertRaises(subprocess.CalledProcessError):
                    reference.capture('owned', run, '/reader.py', ['capture'], interval=0)
        self.assertEqual(calls[-1], ['xdotool', 'keyup', 'l'])
        self.assertNotIn(['capture'], calls)


if __name__ == '__main__':
    unittest.main()
