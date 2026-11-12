#!/usr/bin/env python3
"""Control-signal format, evidence ownership and bounded process cleanup."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch
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

    def test_missing_active_calibration_stream_is_rejected_and_reaped(self):
        with tempfile.TemporaryDirectory() as temp:
            out = Path(temp)
            source = out / 'reference.wav'
            module.reference(source)
            recorder, player = Mock(), Mock()
            recorder.poll.return_value = None
            player.poll.return_value = 0
            player.pid = 123
            def launch(command, **kwargs):
                if command[0] == 'parec':
                    (out / 'monitor.wav').write_bytes(b'x' * 45)
                    return recorder
                return player
            with patch.object(module.subprocess, 'Popen', side_effect=launch), \
                 patch.object(module.subprocess, 'run', return_value=Mock(stdout='', stderr='')), \
                 patch.object(module.time, 'sleep'):
                with self.assertRaisesRegex(RuntimeError, 'stream not observed'):
                    module.run(out, source, 20, warm=False)
            import json
            self.assertFalse(json.loads((out / 'result.json').read_text())['success'])
            recorder.send_signal.assert_called_once()

    def test_stuck_process_is_killed_and_reaped(self):
        process = Mock()
        process.poll.return_value = None
        process.wait.side_effect = [subprocess.TimeoutExpired('recorder', 5), 0]
        module.stop(process)
        process.kill.assert_called_once()
        self.assertEqual(process.wait.call_count, 2)

if __name__ == '__main__':
    unittest.main()
