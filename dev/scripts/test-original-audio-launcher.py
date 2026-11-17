#!/usr/bin/env python3
"""Exercise original audio launch payloads through the public shell command."""
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import unittest


class AudioLauncher(unittest.TestCase):
    def test_busy_live_run_lock_reports_a_bounded_failure_before_launch(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / 'events.json').write_text('[]')
            flock = root / 'flock'
            flock.write_text('#!/bin/sh\nprintf "%s\\n" "$*" > "$LOCK_ARGUMENTS"\nexit 75\n')
            flock.chmod(0o755)
            scripts = root / 'dev/scripts'
            scripts.mkdir(parents=True)
            shutil.copy2(Path(__file__).with_name('original-audio-capture.sh'), scripts)
            runtime = scripts / 'ensure-retro-audio-runtime.sh'
            runtime.write_text('#!/bin/sh\nprintf "%s\\n" cyrus-retro-audio:test\n')
            runtime.chmod(0o755)
            result = subprocess.run(
                ['bash', str(scripts / 'original-audio-capture.sh'),
                 str(root / 'events.json'), str(root / 'output'), '1'],
                capture_output=True, text=True, timeout=3,
                env={**os.environ, 'PATH': f"{root}:{os.environ['PATH']}",
                     'LOCK_ARGUMENTS': str(root / 'lock-arguments'),
                     'CYRUS_RETRO_RUN': str(root / 'run'),
                     'CYRUS_RETRO_GOG': str(root / 'gog')},
            )
            self.assertEqual(result.returncode, 75, result.stderr)
            self.assertIn('original capture: live-run lock unavailable after 30s', result.stderr)
            self.assertEqual((root / 'lock-arguments').read_text().strip(), '-w 30 9')
            self.assertFalse((root / 'output').exists())

    def test_state_reader_can_import_its_region_owner(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            install = root / 'gog/extracted/app/Redguard'
            (install / 'soup386').mkdir(parents=True)
            (install / 'soup386/SOUP386.DEF').write_text('synthetic definition')
            (install / 'system').mkdir()
            (root / 'run/Redguard').mkdir(parents=True)
            (root / 'container').mkdir()
            (root / 'events.json').write_text('[]')
            reader = root / 'reader.py'
            reader.write_text('''import importlib.util, io, pathlib
spec = importlib.util.spec_from_file_location('region', pathlib.Path(__file__).with_name('original-memory-region.py'))
region = importlib.util.module_from_spec(spec)
spec.loader.exec_module(region)
assert region.locate(io.BytesIO(b'___anchor' + bytes(20)), [(0, 29)], b'anchor', 3, 20) == 0
print('{"reader": "bound"}')
''')
            docker = root / 'docker'
            docker.write_text('''#!/usr/bin/env python3
import json, os, pathlib, shutil, subprocess, sys
root = pathlib.Path(os.environ['CAPTURE_TEST'])
args = sys.argv[1:]
with (root / 'docker-calls.jsonl').open('a') as calls:
    calls.write(json.dumps(args) + '\\n')
if args[0] == 'run' and 'sh' in args and os.environ.get('CAPTURE_RECOVERY_FAIL'):
    sys.exit(17)
if args[0] == 'cp':
    shutil.copy2(args[1], root / 'container' / args[2].split(':', 1)[1].rsplit('/', 1)[1])
elif args[0] == 'run' and '-d' in args:
    (root / 'launch.json').write_text(json.dumps(args))
elif args[0] == 'exec' and 'sh' in args and os.environ.get('CAPTURE_CONTAINER_EXITED'):
    sys.exit(1)
elif args[0] == 'exec' and 'python3' in args:
    if os.environ.get('CAPTURE_FAIL'):
        sys.exit(23)
    result = subprocess.run([sys.executable, str(root / 'container/original-state.py')])
    if result.returncode == 0:
        (root / 'run/capture/fixture.wav').write_bytes(b'RIFFfixture')
    sys.exit(result.returncode)
''')
            docker.chmod(0o755)
            flock = root / 'flock'
            flock.write_text('#!/bin/sh\nexit 0\n')
            flock.chmod(0o755)
            scripts = root / 'dev/scripts'
            scripts.mkdir(parents=True)
            for name in ('original-audio-capture.sh', 'prepare-original-definitions.py',
                         'original-capture-inner.py', 'capture-hook.py', 'original-memory-region.py'):
                shutil.copy2(Path(__file__).with_name(name), scripts / name)
            runtime = scripts / 'ensure-retro-audio-runtime.sh'
            runtime.write_text('#!/bin/sh\nprintf "%s\\n" cyrus-retro-audio:test\n')
            runtime.chmod(0o755)
            result = subprocess.run(
                ['bash', str(scripts / 'original-audio-capture.sh'),
                 str(root / 'events.json'), str(root / 'output'), '1'],
                capture_output=True, text=True, timeout=10,
                env={**os.environ, 'PATH': f"{root}:{os.environ['PATH']}",
                     'CAPTURE_TEST': str(root), 'CYRUS_RETRO_RUN': str(root / 'run'),
                     'CYRUS_RETRO_GOG': str(root / 'gog'), 'CYRUS_STATE_READER': str(reader)},
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual((root / 'output/audio.wav').read_bytes(), b'RIFFfixture')
            self.assertIn('bound', (root / 'output/timeline.json').read_text())
            calls = [json.loads(line) for line in (root / 'docker-calls.jsonl').read_text().splitlines()]
            cleanup = [call for call in calls if call[0] == 'exec' and 'sh' in call]
            self.assertEqual(len(cleanup), 1, calls)
            self.assertIn(f'{os.getuid()}:{os.getgid()}', cleanup[0])
            self.assertIn('find /game /capture-config -xdev -user root', cleanup[0][4])
            self.assertEqual(calls[-1][:2], ['rm', '-f'])
            self.assertLess(calls.index(cleanup[0]), len(calls) - 1)
            launch = json.loads((root / 'launch.json').read_text())
            self.assertIn('CYRUS_CONFIG_DIR=/capture-config', launch)
            self.assertIn(f"{root / 'output/config'}:/capture-config", launch)
            (root / 'docker-calls.jsonl').unlink()
            failed = subprocess.run(
                ['bash', str(scripts / 'original-audio-capture.sh'),
                 str(root / 'events.json'), str(root / 'failed-output'), '1'],
                capture_output=True, text=True, timeout=10,
                env={**os.environ, 'PATH': f"{root}:{os.environ['PATH']}",
                     'CAPTURE_TEST': str(root), 'CAPTURE_FAIL': '1',
                     'CYRUS_RETRO_RUN': str(root / 'run'),
                     'CYRUS_RETRO_GOG': str(root / 'gog'), 'CYRUS_STATE_READER': str(reader)},
            )
            self.assertEqual(failed.returncode, 23, failed.stderr)
            calls = [json.loads(line) for line in (root / 'docker-calls.jsonl').read_text().splitlines()]
            self.assertEqual(calls[-2][0], 'exec')
            self.assertIn(f'{os.getuid()}:{os.getgid()}', calls[-2])
            self.assertEqual(calls[-1][:2], ['rm', '-f'])
            (root / 'docker-calls.jsonl').unlink()
            exited = subprocess.run(
                ['bash', str(scripts / 'original-audio-capture.sh'),
                 str(root / 'events.json'), str(root / 'exited-output'), '1'],
                capture_output=True, text=True, timeout=10,
                env={**os.environ, 'PATH': f"{root}:{os.environ['PATH']}",
                     'CAPTURE_TEST': str(root), 'CAPTURE_CONTAINER_EXITED': '1',
                     'CYRUS_RETRO_RUN': str(root / 'run'),
                     'CYRUS_RETRO_GOG': str(root / 'gog'), 'CYRUS_STATE_READER': str(reader)},
            )
            self.assertEqual(exited.returncode, 0, exited.stderr)
            calls = [json.loads(line) for line in (root / 'docker-calls.jsonl').read_text().splitlines()]
            recovery = [call for call in calls if call[0] == 'run' and 'sh' in call]
            self.assertEqual(len(recovery), 1, calls)
            self.assertIn(f'{root / "run"}:/game', recovery[0])
            self.assertIn(f'{root / "exited-output/config"}:/capture-config', recovery[0])
            self.assertIn(f'{os.getuid()}:{os.getgid()}', recovery[0])
            self.assertEqual(calls[-1][:2], ['rm', '-f'])
            broken_cleanup = subprocess.run(
                ['bash', str(scripts / 'original-audio-capture.sh'),
                 str(root / 'events.json'), str(root / 'broken-output'), '1'],
                capture_output=True, text=True, timeout=10,
                env={**os.environ, 'PATH': f"{root}:{os.environ['PATH']}",
                     'CAPTURE_TEST': str(root), 'CAPTURE_CONTAINER_EXITED': '1',
                     'CAPTURE_RECOVERY_FAIL': '1', 'CYRUS_RETRO_RUN': str(root / 'run'),
                     'CYRUS_RETRO_GOG': str(root / 'gog'), 'CYRUS_STATE_READER': str(reader)},
            )
            self.assertNotEqual(broken_cleanup.returncode, 0)
            self.assertIn('could not restore writable output ownership', broken_cleanup.stderr)



if __name__ == '__main__':
    unittest.main()
