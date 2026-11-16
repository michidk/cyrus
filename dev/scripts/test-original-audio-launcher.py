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
if args[0] == 'cp':
    shutil.copy2(args[1], root / 'container' / args[2].split(':', 1)[1].rsplit('/', 1)[1])
elif args[0] == 'run' and '-d' in args:
    (root / 'launch.json').write_text(json.dumps(args))
elif args[0] == 'exec':
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
            launch = json.loads((root / 'launch.json').read_text())
            self.assertIn('CYRUS_CONFIG_DIR=/capture-config', launch)
            self.assertIn(f"{root / 'output/config'}:/capture-config", launch)


if __name__ == '__main__':
    unittest.main()
