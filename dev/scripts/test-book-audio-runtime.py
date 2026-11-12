#!/usr/bin/env python3
"""Runtime recovery must supply rendering and paced PCM together."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('ensure-book-audio-runtime.sh').resolve()

class Runtime(unittest.TestCase):
    def run_case(self, archive, probe=0):
        root = archive.parent
        executable = root / 'docker'
        executable.write_text('''#!/bin/bash
printf '%s\\n' "$*" >> "$COMMANDS"
case "$1" in
 image) exit 1;;
 run) exit "$PROBE";;
 save) touch "$3";;
esac
''')
        executable.chmod(0o755)
        result = subprocess.run(['bash', str(SCRIPT), 'owned-audio:dev', str(archive)],
            env={**os.environ, 'PATH': str(root)+':'+os.environ['PATH'],
                 'COMMANDS': str(root/'commands'), 'PROBE': str(probe)}, capture_output=True)
        return result, (root/'commands').read_text()

    def test_missing_archive_builds_audio_runtime_and_seals_after_probe(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp)/'runtime.tar'
            result, commands = self.run_case(archive)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('Dockerfile.sound-acceptance', commands)
            self.assertIn('lvp_icd.json', commands)
            self.assertIn('parec paplay', commands)
            self.assertIn('libasound_module_pcm_pulse.so', commands)
            self.assertLess(commands.index('run '), commands.index('save '))
            self.assertTrue(archive.exists())

    def test_paced_sink_fixes_the_calibration_sample_domain(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            script = SCRIPT.parents[1] / 'docker/dev/paced-audio.sh'
            source = script.read_text().replace('/run/pulse', str(root/'pulse')).replace('/tmp/cyrus.pa', str(root/'pulse.pa')).replace('/tmp/pulse.log', str(root/'pulse.log')).replace('/etc/asound.conf', str(root/'asound.conf'))
            (root/'paced.sh').write_text(source)
            for name in ['pulseaudio', 'sleep']:
                tool = root/name
                tool.write_text('#!/bin/sh\nexit 0\n'); tool.chmod(0o755)
            result = subprocess.run(['bash', str(root/'paced.sh'), 'true'],
                env={**os.environ, 'PATH': str(root)+':'+os.environ['PATH']}, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('sink_name=cyrus rate=48000 channels=2 format=s16le', (root/'pulse.pa').read_text())
            self.assertIn('type pulse', (root/'asound.conf').read_text())

    def test_incompatible_restored_image_fails_without_overwriting_archive(self):
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp)/'runtime.tar'; archive.write_bytes(b'retained')
            result, commands = self.run_case(archive, 23)
            self.assertEqual(result.returncode, 23)
            self.assertIn('load --input', commands)
            self.assertNotIn('build ', commands)
            self.assertNotIn('save ', commands)
            self.assertEqual(archive.read_bytes(), b'retained')

if __name__ == '__main__': unittest.main()
