#!/usr/bin/env python3
"""A requested hook time is not an observed sound-emission boundary."""
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import time
import unittest

spec = importlib.util.spec_from_file_location('hook', Path(__file__).with_name('capture-hook.py'))
hook = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hook)


class HookBoundary(unittest.TestCase):
    def run_hook(self, root, body):
        path = root / 'hook.py'
        path.write_text(body)
        return hook.CaptureHook([sys.executable, str(path)], root, time.monotonic())

    def test_requested_delay_cannot_acknowledge_an_unfinished_hook(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            capture = self.run_hook(root, 'import time; time.sleep(30)')
            try:
                self.assertIsNone(capture.poll())
                with self.assertRaisesRegex(ValueError, 'unfinished'):
                    capture.finish(required=True)
            finally:
                capture.close()
            self.assertIsNotNone(capture.process.poll())

    def test_failed_hook_retains_stderr_and_cannot_be_admitted(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            capture = self.run_hook(root, 'import sys; print("probe failed", file=sys.stderr); sys.exit(7)')
            capture.process.wait(timeout=3)
            try:
                with self.assertRaisesRegex(ValueError, 'exit 7'):
                    capture.finish(required=True)
                self.assertIn('probe failed', (root / 'hook.stderr').read_text())
            finally:
                capture.close()

    def test_observed_event_retains_its_clock_and_actual_completion(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            capture = self.run_hook(root, 'import json,time; print(json.dumps({"acknowledged":True,"boundary":{"event":"sound_emitted","before":time.monotonic(),"after":time.monotonic(),"game_seconds":3.5,"frame":42,"scene":"fixture","owner":"emitter","effect":9}}))')
            capture.process.wait(timeout=3)
            try:
                result = capture.finish(required=True)
                self.assertTrue(result['acknowledged'])
                self.assertEqual(result['boundary']['frame'], 42)
                self.assertGreaterEqual(result['completed'], result['boundary']['after'])
                self.assertEqual(result['boundary']['effect'], 9)
            finally:
                capture.close()

    def test_hook_emitting_after_recording_stop_cannot_borrow_teardown_time(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            ended = time.monotonic()
            capture = self.run_hook(root, 'import json,time; print(json.dumps({"acknowledged":True,"boundary":{"event":"sound_emitted","before":time.monotonic(),"after":time.monotonic(),"game_seconds":3.5,"frame":42,"scene":"fixture","owner":"emitter","effect":9}}))')
            capture.process.wait(timeout=3)
            try:
                with self.assertRaisesRegex(ValueError, 'no valid observed emission'):
                    capture.finish(required=True, ended=ended)
            finally:
                capture.close()

    def test_poll_time_and_malformed_or_unobserved_events_are_rejected(self):
        for payload in ({}, {'acknowledged': True}, {'acknowledged':True,'boundary':{'event':'input_requested'}}):
            with self.subTest(payload=payload), tempfile.TemporaryDirectory() as temp:
                root = Path(temp)
                capture = self.run_hook(root, f'print({json.dumps(json.dumps(payload))})')
                capture.process.wait(timeout=3)
                try:
                    with self.assertRaises(ValueError):
                        capture.finish(required=True)
                finally:
                    capture.close()


if __name__ == '__main__':
    unittest.main()
