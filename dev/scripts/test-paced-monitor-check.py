#!/usr/bin/env python3
"""Control-signal format, evidence ownership and bounded process cleanup."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock
import wave

spec = importlib.util.spec_from_file_location('monitor', Path(__file__).with_name('paced-monitor-check.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class Controls(unittest.TestCase):
    def test_signal_is_repeatable_stereo_three_seconds(self):
        with tempfile.TemporaryDirectory() as temp:
            a, b = Path(temp)/'a.wav', Path(temp)/'b.wav'
            module.reference(a); module.reference(b)
            self.assertEqual(a.read_bytes(), b.read_bytes())
            with wave.open(str(a)) as source:
                self.assertEqual(source.getparams()[:4], (2, 2, 48000, 144000))

    def test_existing_evidence_is_preserved(self):
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp); (out/'result.json').write_text('prior')
            with self.assertRaisesRegex(ValueError, 'evidence exists'):
                module.run(out, None, 20)
            self.assertEqual((out/'result.json').read_text(), 'prior')

    def test_stuck_process_is_killed_and_reaped(self):
        process = Mock()
        process.poll.return_value = None
        process.wait.side_effect = [subprocess.TimeoutExpired('recorder', 5), 0]
        module.stop(process)
        process.kill.assert_called_once()
        self.assertEqual(process.wait.call_count, 2)

if __name__ == '__main__':
    unittest.main()
