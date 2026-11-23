#!/usr/bin/env python3
"""Check cached retro recovery without Docker or an installed game."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class RetroRecovery(unittest.TestCase):
    def test_build_reuse_and_eviction_restore_share_one_source_key(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scripts = root / 'dev/scripts'
            scripts.mkdir(parents=True)
            shutil.copy2(Path(__file__).with_name('ensure-retro-runtime.sh'), scripts)
            docker = root / 'dev/docker/retro'
            docker.mkdir(parents=True)
            for name in ('Dockerfile', 'entrypoint.sh', 'dosbox-x.conf'):
                (docker / name).write_text(name)
            binary = root / 'bin'
            binary.mkdir()
            fake = binary / 'docker'
            fake.write_text('''#!/bin/sh
printf '%s\\n' "$*" >> "$TEST_ROOT/calls"
case "$1" in
 image) test -f "$TEST_ROOT/present" ;;
 build) touch "$TEST_ROOT/present" ;;
 *) exit 9 ;;
esac
''')
            fake.chmod(0o755)
            runtime = scripts / 'ensure-runtime-image.sh'
            runtime.write_text('''#!/bin/sh
printf '%s\\n' "$*" >> "$TEST_ROOT/calls"
if [ "$1" = --retain ]; then echo archive > "$3"; else touch "$TEST_ROOT/present"; fi
''')
            runtime.chmod(0o755)
            env = dict(os.environ, TEST_ROOT=str(root), PATH=f'{binary}:{os.environ["PATH"]}',
                       CYRUS_MAGIC=str(root / 'host'))
            def run():
                return subprocess.check_output([str(scripts / 'ensure-retro-runtime.sh')],
                                               text=True, env=env).strip()
            image = run()
            self.assertTrue(image.startswith('cyrus-retro:'))
            self.assertIn('build -t', (root / 'calls').read_text())
            (root / 'calls').write_text('')
            self.assertEqual(run(), image)
            self.assertNotIn('build -t', (root / 'calls').read_text())
            (root / 'present').unlink()
            (root / 'calls').write_text('')
            self.assertEqual(run(), image)
            calls = (root / 'calls').read_text()
            self.assertNotIn('build -t', calls)
            self.assertIn(f'{image} ', calls)
            self.assertTrue((root / 'present').exists())
            (docker / 'entrypoint.sh').write_text('changed')
            (root / 'present').unlink()
            self.assertNotEqual(run(), image)
            self.assertIn('build -t', (root / 'calls').read_text())


if __name__ == '__main__':
    unittest.main()
