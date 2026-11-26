#!/usr/bin/env python3
"""Exercise the native build entry point across lost targets and interrupted builds."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest

SCRIPTS = Path(__file__).resolve().parent


class Recovery(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        scripts = self.root / 'dev/scripts'
        scripts.mkdir(parents=True)
        for name in ('build-native-recovery.sh', 'build-native.sh', 'run-game.sh', 'web-build-checkpoint.py',
                     'web-compile-checkpoint.py', 'build-provenance.py', 'cargo-cyrus-native-recovery'):
            shutil.copyfile(SCRIPTS / name, scripts / name)
        (scripts / 'toolchain.sh').write_text('export PATH="$PWD/bin:$PATH"\n')
        (scripts / 'cargo.sh').write_text('cargo() { "$PWD/bin/fake-cargo" "$@"; }\n')
        (self.root / 'bin').mkdir()
        for name, code in {
            'rustc': '#!/bin/sh\ncat "$PWD/compiler"\n',
            'hauler': '#!/bin/sh\nexit 0\n',
            'fake-cargo': '''#!/usr/bin/env python3
import json,os,subprocess
from pathlib import Path
import sys
if sys.argv[1:2] == ['cyrus-native-recovery']:
 sys.exit(subprocess.call(['bash','dev/scripts/cargo-cyrus-native-recovery',*sys.argv[1:]],env={**os.environ,'CARGO_HAULER_INSIDE':'1'}))
if sys.argv[1:] == ['--version']:
 print('cargo-test-version');sys.exit()
root=Path.cwd();target=Path(os.environ['CARGO_TARGET_DIR'])
if (root/'skip-install').exists():print('skipped: no install',flush=True)
(root/'arguments.json').write_text(json.dumps(sys.argv[1:]))
with (root/'cargo.log').open('a') as f:f.write('build\\n')
unit=target/'debug/deps/libdependency-abcd.rlib'
if not unit.exists():
 unit.parent.mkdir(parents=True,exist_ok=True);unit.write_text('completed dependency')
 with (root/'compiled.log').open('a') as f:f.write('dependency\\n')
 print(json.dumps(dict(reason='compiler-artifact',filenames=[str(unit)],package_id='dependency',target=dict(name='dependency'))),flush=True)
if (root/'interrupt').exists():sys.exit(7)
exe=target/'debug/cyrus'
stale=(root/'simulate-shared').exists() and (target/'debug/.fingerprint/cyrus-abcd/bin-cyrus').exists()
exe.write_text('#!/bin/sh\\necho '+('peer' if stale else 'executable')+'\\n');exe.chmod(0o755)
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
        (self.source.parent.parent / 'Cargo.toml').write_text('[package]\nname = \"cyrus\"\nversion = \"0.1.0\"\n')
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

    def test_shared_target_rebuilds_workspace_without_removing_dependency_outputs(self):
        (self.root / 'simulate-shared').touch()
        workspace_stamp = self.target / 'debug/.fingerprint/cyrus-abcd/invoked.timestamp'
        dependency_stamp = self.target / 'debug/.fingerprint/dependency-abcd/invoked.timestamp'
        for stamp in (workspace_stamp, dependency_stamp):
            stamp.parent.mkdir(parents=True)
            stamp.write_text('peer build')
        (workspace_stamp.parent / 'bin-cyrus').write_text('peer fingerprint')
        dependency = self.target / 'debug/deps/libdependency-abcd.rlib'
        dependency.parent.mkdir(parents=True)
        dependency.write_text('retained dependency')
        self.run_build()
        self.assertIn('echo executable', self.output.read_text())
        self.assertTrue(dependency_stamp.exists())
        self.assertEqual(dependency.read_text(), 'retained dependency')
        self.assertFalse((self.root / 'compiled.log').exists())

    def test_interrupted_crate_test_restores_units_without_publishing_game(self):
        (self.root / 'interrupt').touch()
        command = ['bash', 'dev/scripts/build-native-recovery.sh', '--test', 'cyrus-render', 'lighting']
        first = subprocess.run(command, cwd=self.root, env=self.env, capture_output=True, text=True)
        self.assertEqual(first.returncode, 7, first.stdout + first.stderr)
        shutil.rmtree(self.target)
        (self.root / 'interrupt').unlink()
        second = subprocess.run(command, cwd=self.root, env=self.env, capture_output=True, text=True)
        self.assertEqual(second.returncode, 0, second.stdout + second.stderr)
        self.assertEqual((self.root / 'compiled.log').read_text().splitlines(), ['dependency'])
        self.assertFalse(self.output.exists(), 'a test run must not publish a capture executable')
        self.assertEqual(json.loads((self.root / 'arguments.json').read_text()),
                         ['test', '-p', 'cyrus-render', '--message-format=json-render-diagnostics',
                          'lighting', '--', '--nocapture'])
        third = subprocess.run(command, cwd=self.root, env=self.env, capture_output=True, text=True)
        self.assertEqual(third.returncode, 0, third.stdout + third.stderr)
        self.assertEqual(len((self.root / 'cargo.log').read_text().splitlines()), 3,
                         'retained compilation must never replace execution of the tests')

    def test_test_recovery_rejects_install_skips_when_an_install_is_present(self):
        install = self.root / 'install'
        install.mkdir()
        (install / 'RGFX.EXE').write_text('installed fixture')
        (self.root / 'skip-install').touch()
        command = ['bash', 'dev/scripts/build-native-recovery.sh', '--test', 'cyrus-script']
        result = subprocess.run(command, cwd=self.root,
                                env={**self.env, 'REDGUARD_DIR': str(install)},
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn('install tests skipped', result.stderr)
        self.assertFalse(self.output.exists())
        (install / 'RGFX.EXE').unlink()
        result = subprocess.run(command, cwd=self.root,
                                env={**self.env, 'REDGUARD_DIR': str(install)},
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('skipped: no install', result.stdout)

    def test_library_recovery_selects_only_the_library_target(self):
        command = ['bash', 'dev/scripts/build-native-recovery.sh', '--test-lib',
                   'cyrus-startup', 'first_book_publication']
        for _ in range(2):
            result = subprocess.run(command, cwd=self.root, env=self.env,
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertEqual(json.loads((self.root / 'arguments.json').read_text()),
                             ['test', '-p', 'cyrus-startup', '--message-format=json-render-diagnostics',
                              '--lib', 'first_book_publication', '--', '--nocapture'])
            self.assertFalse(self.output.exists())
            shutil.rmtree(self.target)
        self.assertEqual((self.root / 'compiled.log').read_text().splitlines(), ['dependency'])
        self.assertEqual(len((self.root / 'cargo.log').read_text().splitlines()), 2)

    def test_target_restore_runs_inside_the_same_hauler_lane_as_compilation(self):
        recorder = self.root / 'dev/scripts/web-compile-checkpoint.py'
        recorder.write_text("import os,sys\n"
                            "if sys.argv[1] == 'restore' and not os.environ.get('CARGO_HAULER_INSIDE'):\n"
                            " sys.exit('target restore ran outside the hauler lane')\n" + recorder.read_text())
        self.run_build()

    def test_internal_subcommand_rejects_execution_without_hauler(self):
        result = subprocess.run(['bash', 'dev/scripts/cargo-cyrus-native-recovery',
                                 'cyrus-native-recovery', str(self.output)],
                                cwd=self.root, env={k: v for k, v in self.env.items()
                                                   if k != 'CARGO_HAULER_INSIDE'},
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('requires the hauler lane', result.stderr)
        self.assertFalse((self.root / 'cargo.log').exists())

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

    def test_result_receipt_survives_failure_and_target_loss(self):
        (self.root / 'interrupt').touch()
        self.run_build(7)
        receipt = self.root / 'retained/result.json'
        failed = json.loads(receipt.read_text())
        self.assertEqual((failed['status'], failed['exit_code']), ('failed', 7))
        self.assertIn('sha256', failed['source'])
        # Simulate an uncatchable runtime loss after the running receipt.
        failed['status'] = 'running'
        receipt.write_text(json.dumps(failed))
        shutil.rmtree(self.target)
        (self.root / 'interrupt').unlink()
        resumed = self.run_build()
        self.assertIn('previous native attempt interrupted', resumed.stderr)
        completed = json.loads(receipt.read_text())
        self.assertEqual((completed['status'], completed['exit_code']), ('succeeded', 0))
        self.assertEqual(completed['source'], failed['source'])
        self.assertEqual(len(completed['executable_sha256']), 64)
        self.output.unlink()
        shutil.rmtree(self.target)
        self.run_build()
        self.assertEqual(len((self.root / 'cargo.log').read_text().splitlines()), 2)

    def test_public_build_preserves_target_and_recovers_with_capture_provenance(self):
        self.env['CYRUS_NATIVE_EXECUTABLE'] = str(self.output)
        for _ in range(2):
            result = subprocess.run(['bash', 'dev/scripts/build-native.sh'],
                                    cwd=self.root, env=self.env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertTrue(self.output.is_file(), 'public build did not retain the executable')
            record = Path(str(self.output) + '.provenance.json')
            subprocess.run(['python3', 'dev/scripts/build-provenance.py', 'verify', str(record)],
                           cwd=self.root, check=True, capture_output=True)
            self.assertEqual(list(json.loads(record.read_text())['artifacts']), [str(self.output)])
            shutil.rmtree(self.target, ignore_errors=True)
            self.output.unlink()
        self.assertEqual((self.root / 'cargo.log').read_text().splitlines(), ['build'])
        self.assertFalse((self.root / 'target').exists(), 'public build overrode the inherited target')

    def test_public_launcher_mounts_the_retained_executable(self):
        self.env['CYRUS_NATIVE_EXECUTABLE'] = str(self.output)
        self.env['REDGUARD_DIR'] = str(self.root / 'install')
        (self.root / 'install').mkdir()
        (self.root / 'game.gog').write_bytes(b'CD image fixture')
        (self.root / 'game.ins').write_text('FILE "game.gog" BINARY\n')
        docker = self.root / 'bin/docker'
        docker.write_text("#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$PWD/docker.log\"\n")
        docker.chmod(0o755)
        result = subprocess.run(['bash', 'dev/scripts/run-game.sh', '--scene', 'TEST'],
                                cwd=self.root, env={**self.env, 'PATH': str(self.root / 'bin') + ':' + os.environ['PATH']},
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = (self.root / 'docker.log').read_text().splitlines()
        self.assertIn(str(self.output) + ':/app/cyrus:ro', calls)
        self.assertIn('/app/cyrus', calls)
        self.assertIn(str(self.root / 'install/../game.gog') + ':/game.gog:ro', calls)
        self.assertIn(str(self.root / 'install/../game.ins') + ':/game.ins:ro', calls)
        self.assertEqual(calls[-2:], ['--scene', 'TEST'])
        self.assertFalse((self.root / 'target').exists())
        self.assertIn('CYRUS_CONFIG_DIR=/repo/game-config', calls)
        self.env['CYRUS_CONFIG_DIR'] = str(self.root / 'run state/config')
        (self.root / 'docker.log').unlink()
        result = subprocess.run(['bash', 'dev/scripts/run-game.sh', '--scene', 'TEST'],
                                cwd=self.root, env={**self.env, 'PATH': str(self.root / 'bin') + ':' + os.environ['PATH']},
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        calls = (self.root / 'docker.log').read_text().splitlines()
        self.assertIn(str(self.root / 'run state/config') + ':/cyrus-config', calls)
        self.assertIn('CYRUS_CONFIG_DIR=/cyrus-config', calls)
        self.assertNotIn('CYRUS_CONFIG_DIR=/repo/game-config', calls)
        self.assertTrue((self.root / 'run state/config').is_dir())

    def test_completed_executable_is_sealed_before_slow_unit_recording(self):
        recorder = self.root / 'dev/scripts/web-compile-checkpoint.py'
        original = recorder.read_text()
        recorder.write_text("import sys,time\nfrom pathlib import Path\n"
                            "if sys.argv[1] == 'record':\n"
                            " while not Path('release-recorder').exists(): time.sleep(.02)\n" + original)
        # The shim is a fixture, not a modification made during the build.
        process = subprocess.Popen(['bash', 'dev/scripts/build-native-recovery.sh', str(self.output)],
                                   cwd=self.root, env=self.env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        try:
            deadline = time.monotonic() + 4
            record = Path(str(self.output) + '.provenance.json')
            while not record.exists() and time.monotonic() < deadline:
                time.sleep(.02)
            self.assertTrue(record.exists(), 'completed executable waited behind unit checkpoint recording')
            subprocess.run(['python3', 'dev/scripts/build-provenance.py', 'verify', str(record)],
                           cwd=self.root, check=True, capture_output=True)
        finally:
            (self.root / 'release-recorder').touch()
            stdout, stderr = process.communicate(timeout=15)
        self.assertEqual(process.returncode, 0, stdout + stderr)

    def test_final_publication_stops_competing_unit_recording(self):
        recorder = self.root / 'dev/scripts/web-compile-checkpoint.py'
        recorder.write_text("import sys,time,signal\nfrom pathlib import Path\n"
                            "if sys.argv[1] == 'record':\n"
                            " marker=Path('recorder-active');marker.touch()\n"
                            " def stop(*args): marker.unlink();sys.exit(0)\n"
                            " signal.signal(signal.SIGTERM,stop)\n"
                            " while True: time.sleep(.01)\n" + recorder.read_text())
        cargo = self.root / 'bin/fake-cargo'
        cargo.write_text(cargo.read_text().replace(
            "exe=target/'debug/cyrus'", "import time\n"
            "while not (root/'recorder-active').exists(): time.sleep(.01)\n"
            "exe=target/'debug/cyrus'"))
        checkpoint = self.root / 'dev/scripts/web-build-checkpoint.py'
        checkpoint.write_text("import sys\nfrom pathlib import Path\n"
                              "if sys.argv[1] == 'store' and Path('recorder-active').exists():\n"
                              " sys.exit('final executable publication competes with unit recording')\n"
                              + checkpoint.read_text())
        self.run_build()
        self.assertTrue(Path(str(self.output) + '.provenance.json').exists())

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
