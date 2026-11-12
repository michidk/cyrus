#!/usr/bin/env python3
import importlib.util
from pathlib import Path
import tempfile
import unittest
import wave
import struct
from unittest.mock import patch
import json

spec = importlib.util.spec_from_file_location('capture', Path(__file__).with_name('paced-mixer-capture.py'))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)

class Acceptance(unittest.TestCase):
    def fixture(self, path, silent=False):
        with wave.open(str(path), 'wb') as out:
            out.setparams((2, 2, 48000, 0, 'NONE', 'not compressed'))
            out.writeframes(struct.pack('<hh', *([0, 0] if silent else [1000, -1000])) * 48000)

    def test_real_pcm_and_acknowledged_transition(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'audio.wav'; self.fixture(path)
            result = capture.validate(path, 1, [{'sent': .1, 'acknowledged': .3, 'from': 0, 'to': 4}], .1)
            self.assertEqual(result['peak'], 1000)
            self.assertEqual(result['frames'], 48000)

    def test_silence_is_not_audible_acceptance(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'audio.wav'; self.fixture(path, True)
            with self.assertRaisesRegex(ValueError, 'silent'):
                capture.validate(path, 1, [{'sent': .1, 'acknowledged': .3, 'from': 0, 'to': 4}], .1)

    def test_fast_drain_and_unacknowledged_inputs_fail(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / 'audio.wav'; self.fixture(path)
            event = {'sent': .1, 'acknowledged': .3, 'from': 0, 'to': 4}
            with self.assertRaisesRegex(ValueError, 'pacing'):
                capture.validate(path, 3, [event], .1)
            for events in ([], [dict(event, acknowledged=None)], [dict(event, acknowledged=.05)], [dict(event, to=0)]):
                with self.assertRaises(ValueError):
                    capture.validate(path, 1, events, .1)

    def test_boot_screen_is_not_ready(self):
        with self.assertRaisesRegex(ValueError, 'book is not ready'):
            capture.require_ready({'ready': False, 'page': None})
        with self.assertRaisesRegex(ValueError, 'book is not ready'):
            capture.require_ready({'ready': True})
        self.assertEqual(capture.require_ready({'ready': True, 'page': 0})['page'], 0)

    def test_readiness_timeout_preserves_failure_without_recording(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            with patch.object(capture.subprocess, 'run', side_effect=capture.subprocess.TimeoutExpired(['ready'], 1)), patch.object(capture.subprocess, 'Popen') as recorder:
                with self.assertRaises(capture.subprocess.TimeoutExpired):
                    capture.capture(path, ['driver'], 1, .1, ['ready'])
                recorder.assert_not_called()
            self.assertFalse(json.loads((path / 'result.json').read_text())['success'])
            self.assertFalse((path / 'audio.wav').exists())

    def test_failed_readiness_retains_diagnostic_before_recording(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            ready = capture.subprocess.CompletedProcess(['ready'], 1, '', 'display not ready')
            with patch.object(capture.subprocess, 'run', return_value=ready), patch.object(capture.subprocess, 'Popen') as recorder:
                with self.assertRaises(capture.subprocess.CalledProcessError):
                    capture.capture(path, ['driver'], 1, .1, ['ready'])
                recorder.assert_not_called()
            self.assertEqual((path / 'readiness.log').read_text(), 'display not ready')

    def test_monitor_requests_bounded_drain_latency(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            ready = capture.subprocess.CompletedProcess(['ready'], 0, '{"ready":true,"page":0}', '')
            with patch.object(capture.subprocess, 'run', return_value=ready), patch.object(capture.subprocess, 'Popen', side_effect=FileNotFoundError('control')) as start:
                with self.assertRaises(FileNotFoundError):
                    capture.capture(path, ['driver'], 1, .1, ['ready'])
            self.assertIn('--latency-msec=20', start.call_args.args[0])

    def test_missing_monitor_preserves_failure(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp)
            with patch.object(capture.subprocess, 'run', return_value=type('Ready', (), {'stdout': '{"ready":true,"page":0}', 'stderr': '', 'check_returncode': lambda self: None})()), patch.object(capture.subprocess, 'Popen', side_effect=FileNotFoundError('parec absent')):
                with self.assertRaises(FileNotFoundError):
                    capture.capture(path, ['driver'], 1, .1, ['ready'])
            record = json.loads((path / 'result.json').read_text())
            self.assertFalse(record['success'])
            self.assertIn('parec absent', record['error'])

if __name__ == '__main__':
    unittest.main()
