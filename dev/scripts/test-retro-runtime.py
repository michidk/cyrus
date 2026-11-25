#!/usr/bin/env python3
"""Check cached retro recovery without Docker or an installed game."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


class RetroRecovery(unittest.TestCase):
    def test_explicit_retained_runtime_reuses_its_archive_without_build_or_rewrite(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scripts = root / 'dev/scripts'
            scripts.mkdir(parents=True)
            shutil.copy2(Path(__file__).with_name('ensure-retro-runtime.sh'), scripts)
            archive = root / 'retained.tar.gz'
            archive.write_bytes(b'retained compressed runtime')
            helper = scripts / 'ensure-runtime-image.sh'
            helper.write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "$TEST_CALLS"\n')
            helper.chmod(0o755)
            calls = root / 'calls'
            env = dict(os.environ, TEST_CALLS=str(calls), CYRUS_RETRO_IMAGE='retained:verified',
                       CYRUS_RETRO_IMAGE_ARCHIVE=str(archive))
            image = subprocess.check_output([str(scripts / 'ensure-retro-runtime.sh')],
                                            env=env, text=True).strip()
            self.assertEqual(image, 'retained:verified')
            self.assertEqual(calls.read_text().splitlines(), ['retained:verified', str(archive)])
            self.assertEqual(archive.read_bytes(), b'retained compressed runtime')

    def test_incomplete_explicit_runtime_is_rejected(self):
        script = Path(__file__).with_name('ensure-retro-runtime.sh')
        for present in ('CYRUS_RETRO_IMAGE', 'CYRUS_RETRO_IMAGE_ARCHIVE'):
            env = {key: value for key, value in os.environ.items()
                   if key not in ('CYRUS_RETRO_IMAGE', 'CYRUS_RETRO_IMAGE_ARCHIVE')}
            env[present] = 'fixture'
            result = subprocess.run([str(script)], env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 2)
            self.assertIn('requires both', result.stderr)

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
            env.pop('CYRUS_RETRO_IMAGE', None)
            env.pop('CYRUS_RETRO_IMAGE_ARCHIVE', None)
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
