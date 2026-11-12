#!/usr/bin/env python3
"""Drive the recorder protocol with a failing real hook subprocess."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class OriginalHook(unittest.TestCase):
    def test_failed_hook_cannot_publish_a_successful_emission_timeline(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            hook = root / 'probe.py'
            hook.write_text('import sys; print("emission probe failed", file=sys.stderr); sys.exit(7)')
            inner = Path(__file__).with_name('original-capture-inner.py')
            runner = root / inner.name
            # Keep the recorder's actual hook path isolated from other sessions.
            runner.write_text(inner.read_text().replace('/tmp/hook.py', str(hook)))
            helper = inner.with_name('capture-hook.py')
            if helper.exists():
                (root / helper.name).write_bytes(helper.read_bytes())
            reader = root / 'reader.py'
            reader.write_text('print(\'{"counters":{"mainloop_frame":401}}\')')
            (root / 'events.json').write_text('[]')
            tool = root / 'xdotool'
            tool.write_text('#!/bin/sh\nexit 0\n'); tool.chmod(0o755)
            result = subprocess.run([sys.executable, str(runner), str(reader), str(root / 'events.json'), '1.1'],
                env={**os.environ, 'PATH':f"{root}:{os.environ['PATH']}", 'CYRUS_CAPTURE_DIR':str(root),
                     'CYRUS_REQUIRE_HOOK_ACK':'1'}, capture_output=True, text=True, timeout=6)
            self.assertNotEqual(result.returncode, 0, 'failed emission hook was admitted as successful capture')
            self.assertNotIn('"hook", "placement"', result.stdout)
            self.assertTrue((root / 'hook-result.json').exists(), result.stderr)
            self.assertFalse(json.loads((root / 'hook-result.json').read_text())['acknowledged'])

    def test_observed_game_progress_stops_the_recording_without_wall_scaling(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            reader = root / 'reader.py'
            reader.write_text('import pathlib,json; p=pathlib.Path(__file__).with_name("clock"); n=int(p.read_text())+1 if p.exists() else 0; p.write_text(str(n)); print(json.dumps({"counters":{"mainloop_frame":401+n},"game_seconds":n/4}))')
            (root / 'events.json').write_text('[]')
            tool = root / 'xdotool'
            tool.write_text('#!/bin/sh\nexit 0\n'); tool.chmod(0o755)
            result = subprocess.run([sys.executable, str(Path(__file__).with_name('original-capture-inner.py')),
                str(reader), str(root / 'events.json'), '30'],
                env={**os.environ, 'PATH':f"{root}:{os.environ['PATH']}", 'CYRUS_CAPTURE_DIR':str(root),
                     'CYRUS_CAPTURE_GAME_SECONDS':'.5'}, capture_output=True, text=True, timeout=6)
            self.assertEqual(result.returncode, 0, result.stderr)
            timeline = json.loads(result.stdout)
            self.assertEqual(timeline['clock_domain'], 'emulated_mixer')
            self.assertEqual(timeline['stop']['reason'], 'game-progress')
            self.assertGreaterEqual(timeline['stop']['observed_game_progress'], .5)



if __name__ == '__main__':
    unittest.main()
