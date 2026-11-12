#!/usr/bin/env python3
"""Exercise recovery, run ownership and failure cleanup without Docker or game data."""
from pathlib import Path
import os
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('original-book-audio.sh').resolve()

class Recovery(unittest.TestCase):
    def test_capture_failure_cleans_only_its_run_and_retains_host_paths(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ['run', 'gog', 'bin']:
                (root/name).mkdir()
            for name in ['image.tar', 'ready.py', 'driver.py']:
                (root/name).write_text('local test fixture')
            (root/'run/Redguard').mkdir()
            original_ini = b'[system]\r\nredbook=on\r\nredbook_volume=200\r\n'
            system_ini = root/'run/Redguard/SYSTEM.INI'
            system_ini.write_bytes(original_ini)
            docker = root/'bin/docker'
            docker.write_text('''#!/bin/bash
printf '%s\\n' "$*" >> "$DOCKER_LOG"
case "$1" in
 image) exit 0;;
 run) echo owned-container;;
 exec) exit 23;;
esac
''')
            docker.chmod(0o755)
            out = root/'evidence'
            result = subprocess.run(['bash', str(SCRIPT), str(root/'run'), str(root/'gog'),
                str(out), str(root/'image.tar'), str(root/'ready.py'), str(root/'driver.py')],
                env={**os.environ, 'PATH': str(root/'bin')+':'+os.environ['PATH'],
                     'DOCKER_LOG': str(root/'docker.log'),
                     'CYRUS_BOOK_REALTIME': '1', 'CYRUS_BOOK_ISOLATE_MUSIC': '1'}, capture_output=True, text=True)
            self.assertEqual(result.returncode, 23, result.stderr)
            commands = (root/'docker.log').read_text().splitlines()
            run = next(c for c in commands if c.startswith('run '))
            name = run.split('--name ')[1].split()[0]
            self.assertIn(f'-v {out}:/evidence', run)
            self.assertIn('CYRUS_CONFIG_DIR=/evidence/config', run)
            self.assertIn('bash /evidence/runtime/entrypoint.sh', run)
            self.assertIn('-set cpu cycles=max', run)
            self.assertIn('-set dosbox hostkey=ctrlalt', run)
            self.assertEqual(system_ini.read_bytes(), original_ini)
            self.assertEqual((out/'runtime/SYSTEM.capture.ini').read_bytes(), original_ini.replace(b'redbook=on', b'redbook=off'))
            self.assertIn(f'-v {out}/runtime/SYSTEM.capture.ini:/game/Redguard/SYSTEM.INI', run)
            self.assertTrue((root/'run/.cyrus-book-audio.lock').exists())
            self.assertIn(f'rm -f {name}', commands)
            self.assertTrue((out/'runtime/ready.py').exists())
            self.assertTrue((out/'container.log').exists())
            (out/'capture').mkdir(); (out/'capture/result.json').write_text('{}')
            repeated = subprocess.run(['bash', str(SCRIPT), str(root/'run'), str(root/'gog'),
                str(out), str(root/'image.tar'), str(root/'ready.py'), str(root/'driver.py')],
                capture_output=True, text=True)
            self.assertNotEqual(repeated.returncode, 0)
            self.assertIn('preserve evidence', repeated.stderr)

    def test_music_isolation_rejects_linked_or_unsupported_config(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ['run/Redguard', 'gog', 'bin']:
                (root/name).mkdir(parents=True)
            for name in ['image.tar', 'ready.py', 'driver.py']:
                (root/name).write_text('local test fixture')
            docker = root/'bin/docker'
            docker.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$DOCKER_LOG"\n')
            docker.chmod(0o755)
            canonical = root/'canonical.ini'
            canonical.write_bytes(b'redbook=on\r\n')
            ini = root/'run/Redguard/SYSTEM.INI'
            ini.symlink_to(canonical)
            env = {**os.environ, 'PATH': str(root/'bin')+':'+os.environ['PATH'],
                   'DOCKER_LOG': str(root/'docker.log'), 'CYRUS_BOOK_ISOLATE_MUSIC': '1'}
            for case in ['linked', 'unsupported']:
                if case == 'unsupported':
                    ini.unlink()
                    ini.write_bytes(b'redbook=unknown\r\n')
                before = ini.read_bytes()
                result = subprocess.run(['bash', str(SCRIPT), str(root/'run'), str(root/'gog'),
                    str(root/case), str(root/'image.tar'), str(root/'ready.py'), str(root/'driver.py')],
                    env=env, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertNotIn('run ', (root/'docker.log').read_text())
                self.assertEqual(ini.read_bytes(), before)
            self.assertEqual(canonical.read_bytes(), b'redbook=on\r\n')

    def test_continuity_failure_is_not_successful_capture(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            for name in ['run', 'gog', 'bin']:
                (root/name).mkdir()
            for name in ['image.tar', 'ready.py', 'driver.py', 'reference.wav']:
                (root/name).write_text('local fixture')
            docker = root/'bin/docker'
            docker.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$DOCKER_LOG"\nexit 0\n')
            docker.chmod(0o755)
            analysis = root/'bin/analysis'
            analysis.write_text('#!/bin/sh\nprintf "%s\\n" "$*" > "$ANALYSIS_LOG"\nexit 24\n')
            analysis.chmod(0o755)
            result = subprocess.run(['bash', str(SCRIPT), str(root/'run'), str(root/'gog'),
                str(root/'evidence'), str(root/'image.tar'), str(root/'ready.py'),
                str(root/'driver.py'), str(root/'reference.wav')],
                env={**os.environ, 'PATH': str(root/'bin')+':'+os.environ['PATH'],
                     'DOCKER_LOG': str(root/'docker.log'),
                     'ANALYSIS_LOG': str(root/'analysis.log'),
                     'CYRUS_AUDIO_ANALYSIS_PYTHON': str(analysis)}, capture_output=True, text=True)
            self.assertEqual(result.returncode, 24, result.stderr)
            self.assertIn('--reference '+str(root/'reference.wav'), (root/'analysis.log').read_text())
            self.assertIn('rm -f book-audio-', (root/'docker.log').read_text())

if __name__ == '__main__':
    unittest.main()
