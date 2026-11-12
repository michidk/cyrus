#!/usr/bin/env python3
"""Calibration isolation restores the actual owned player on every exit."""
import fcntl
import sys
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import Mock, patch

spec = importlib.util.spec_from_file_location('paired', Path(__file__).with_name('paired-book-audio.py'))
paired = importlib.util.module_from_spec(spec)
spec.loader.exec_module(paired)


def stream(binary='owned', **changes):
    return dict(index=7, sink=0, mute=False, volume={'left': {'value': 65536}},
                sample_specification='s16le 2ch 48000Hz', channel_map='left,right',
                properties={'application.process.binary': binary}, **changes)


class Pairing(unittest.TestCase):
    def test_public_capture_rejects_an_already_owned_recording(self):
        with tempfile.TemporaryDirectory() as temp:
            with (Path(temp)/'pair.lock').open('a') as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                result=subprocess.run([sys.executable, str(Path(paired.__file__)),
                    '--producer','owned','--clock-command','unused',
                    '--ready-command','unused','capture',temp,'--','unused'],capture_output=True,text=True)
                self.assertNotEqual(result.returncode,0)
                self.assertIn('BlockingIOError',result.stderr)
                self.assertFalse((Path(temp)/'capture').exists())

    def test_requires_one_unmuted_exact_producer(self):
        value = stream()
        for values in [[], [value, value], [dict(value, mute=True)], [stream('peer')]]:
            with self.assertRaises(ValueError): paired.producer(values, 'owned')
        self.assertEqual(paired.producer([value, stream('peer')], 'owned'), value)

    def test_failed_signal_restores_only_owned_stream_and_retains_settings(self):
        with tempfile.TemporaryDirectory() as temp:
            monitor = Mock(); monitor.run.side_effect = RuntimeError('signal failure')
            with patch.object(paired, 'streams', return_value=[stream(), stream('peer')]), \
                 patch.object(paired, 'module', return_value=monitor), \
                 patch.object(paired.subprocess, 'run') as commands:
                with self.assertRaisesRegex(RuntimeError, 'signal failure'):
                    paired.calibrate(Path(temp), 'owned')
            self.assertEqual([call.args[0] for call in commands.call_args_list],
                [['pactl', 'set-sink-input-mute', '7', '1'], ['pactl', 'set-sink-input-mute', '7', '0']])
            value = json.loads((Path(temp)/'calibration-settings.json').read_text())
            self.assertEqual(value['before'], value['after'])

    def test_changed_volume_cannot_pair_calibration(self):
        with tempfile.TemporaryDirectory() as temp:
            before = stream(); after = dict(before, volume={'left': {'value': 32768}})
            with patch.object(paired, 'streams', side_effect=[[before], [after]]), \
                 patch.object(paired, 'module', return_value=Mock()), patch.object(paired.subprocess, 'run'):
                with self.assertRaisesRegex(ValueError, 'settings changed'):
                    paired.calibrate(Path(temp), 'owned')

    def test_stuck_observer_cannot_admit_an_extended_audio_tail(self):
        with tempfile.TemporaryDirectory() as temp:
            observer=Mock();observer.poll.return_value=None
            observer.wait.side_effect=[subprocess.TimeoutExpired('clock',5),0]
            with patch.object(paired,'calibrate'),patch.object(paired.subprocess,'Popen',return_value=observer),patch.object(paired.subprocess,'run'):
                with self.assertRaisesRegex(RuntimeError,'did not stop'):
                    paired.drive(Path(temp),'owned','clock',['input'])
            observer.kill.assert_called_once()

    def test_transition_failure_stops_observer_without_awaiting_fixed_duration(self):
        with tempfile.TemporaryDirectory() as temp:
            observer = Mock(); observer.poll.return_value = None
            with patch.object(paired, 'calibrate'), \
                 patch.object(paired.subprocess, 'Popen', return_value=observer), \
                 patch.object(paired.subprocess, 'run', side_effect=subprocess.CalledProcessError(3, ['input'])):
                with self.assertRaises(subprocess.CalledProcessError):
                    paired.drive(Path(temp), 'owned', 'clock', ['input'])
            observer.terminate.assert_called_once()
            observer.wait.assert_called_once_with(timeout=5)

if __name__ == '__main__': unittest.main()
