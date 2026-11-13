#!/usr/bin/env python3
"""Exercise the native build entry point across lost targets and interrupted builds."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parent


class Recovery(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        scripts = self.root / 'dev/scripts'
        scripts.mkdir(parents=True)
        for name in ('build-native-recovery.sh', 'web-build-checkpoint.py',
                     'web-compile-checkpoint.py', 'build-provenance.py'):
            shutil.copyfile(SCRIPTS / name, scripts / name)
        (scripts / 'toolchain.sh').write_text('export PATH="$PWD/bin:$PATH"\n')
        (scripts / 'cargo.sh').write_text('cargo() { "$PWD/bin/fake-cargo" "$@"; }\n')
        (self.root / 'bin').mkdir()
        for name, code in {
            'rustc': '#!/bin/sh\ncat "$PWD/compiler"\n',
            'hauler': '#!/bin/sh\nexit 0\n',
            'fake-cargo': '''#!/usr/bin/env python3
import json,os
from pathlib import Path
import sys
if sys.argv[1:] == ['--version']:
 print('cargo-test-version');sys.exit()
root=Path.cwd();target=Path(os.environ['CARGO_TARGET_DIR'])
with (root/'cargo.log').open('a') as f:f.write('build\\n')
unit=target/'debug/deps/libdependency-abcd.rlib'
if not unit.exists():
 unit.parent.mkdir(parents=True,exist_ok=True);unit.write_text('completed dependency')
 with (root/'compiled.log').open('a') as f:f.write('dependency\\n')
 print(json.dumps(dict(reason='compiler-artifact',filenames=[str(unit)],package_id='dependency',target=dict(name='dependency'))),flush=True)
if (root/'interrupt').exists():sys.exit(7)
exe=target/'debug/cyrus';exe.write_text('#!/bin/sh\\necho executable\\n');exe.chmod(0o755)
print(json.dumps(dict(reason='compiler-artifact',filenames=[str(exe)],package_id='cyrus',target=dict(name='cyrus'))),flush=True)
if (root/'source-race').exists(): (root/'crates/cyrus/src/main.rs').write_text('changed during build')
''',
        }.items():
            path = self.root / 'bin' / name
            path.write_text(code)
            path.chmod(0o755)
        self.source = self.root / 'crates/cyrus/src/main.rs'
        self.source.parent.mkdir(parents=True)
        self.source.write_text('source')
        (self.root / 'compiler').write_text('compiler-one')
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        (self.root / '.git/HEAD').write_text('1' * 40 + '\n')
        self.target = self.root / 'ephemeral'
        self.output = self.root / 'application/cyrus'
        self.env = {**os.environ, 'CARGO_TARGET_DIR': str(self.target),
                    'CYRUS_NATIVE_CHECKPOINT_DIR': str(self.root / 'retained')}

    def run_build(self, expected=0):
        result = subprocess.run(['bash', 'dev/scripts/build-native-recovery.sh', str(self.output)],
                                cwd=self.root, env=self.env, capture_output=True, text=True)
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        return result

    def test_interrupted_build_restores_completed_units_and_final_executable(self):
        (self.root / 'interrupt').touch()
        self.run_build(7)
        self.assertFalse(self.output.exists())
        shutil.rmtree(self.target)
        (self.root / 'interrupt').unlink()
        self.run_build()
        self.assertEqual((self.root / 'compiled.log').read_text().splitlines(), ['dependency'])
        self.assertEqual(subprocess.check_output([self.output], text=True).strip(), 'executable')
        shutil.rmtree(self.target)
        self.output.unlink()
        self.run_build()
        self.assertEqual(len((self.root / 'cargo.log').read_text().splitlines()), 2)
        self.assertTrue(os.access(self.output, os.X_OK))

    def test_changed_source_and_compiler_reject_reuse(self):
        self.run_build()
        for path in (self.source, self.root / 'compiler'):
            shutil.rmtree(self.target)
            path.write_text('changed')
            self.run_build()
        self.assertEqual(len((self.root / 'compiled.log').read_text().splitlines()), 3)

    def test_corrupt_sealed_executable_is_rejected(self):
        self.run_build()
        (self.root / 'retained/cyrus').write_text('corrupt')
        result = subprocess.run(['bash', 'dev/scripts/build-native-recovery.sh', str(self.output)],
                                cwd=self.root, env=self.env, capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('corrupt build checkpoint', result.stderr)
        self.assertEqual(len((self.root / 'cargo.log').read_text().splitlines()), 1)

    def test_source_race_cannot_publish_executable(self):
        (self.root / 'source-race').touch()
        self.run_build(1)
        self.assertFalse(self.output.exists())
        self.assertFalse((self.root / 'retained/cyrus.json').exists())

    def test_final_restore_cannot_replace_shared_cargo_artifacts(self):
        self.output = self.target / 'debug/cyrus'
        self.run_build(1)
        self.assertFalse((self.root / 'cargo.log').exists())

    def test_unavailable_daemon_cannot_fall_back_to_unqueued_cargo(self):
        (self.root / 'bin/hauler').write_text('#!/bin/sh\nexit 4\n')
        self.run_build(4)
        self.assertFalse((self.root / 'cargo.log').exists())

    def test_absent_hauler_cannot_fall_back_to_unqueued_cargo(self):
        (self.root / 'bin/hauler').unlink()
        # Do not discover the real shared daemon in the test environment.
        (self.root / 'dev/scripts/toolchain.sh').write_text('export PATH="$PWD/bin:/usr/bin:/bin"\n')
        self.run_build(1)
        self.assertFalse((self.root / 'cargo.log').exists())


if __name__ == '__main__':
    unittest.main()
