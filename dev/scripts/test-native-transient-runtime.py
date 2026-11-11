#!/usr/bin/env python3
"""Controls for runtime recovery and durable acceptance resume."""
import importlib.util
import io
from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import patch


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


runtime = load('native-transient-runtime')
transient = load('native-transient-session')


class RecoveryControls(unittest.TestCase):
    def test_gpu_mode_requires_seed_before_starting_runtime(self):
        import subprocess
        result = subprocess.run([
            __import__('sys').executable, str(Path(runtime.__file__)), '--gpu-pixels',
            '--runtime', '/unused', '--binary', '/unused/cyrus', '--sha256', 'unused',
            '--install', '/unused', '--cache', '/unused'], capture_output=True, text=True)
        self.assertEqual(result.returncode, 2)
        self.assertIn('--gpu-pixels requires --seed-session', result.stderr)

    def test_missing_image_builds_only_runtime_and_existing_image_is_reused(self):
        for status in (0, 1):
            with self.subTest(status=status), patch.object(runtime.subprocess, 'run',
                    return_value=SimpleNamespace(returncode=status)), patch.object(runtime, 'docker') as docker:
                runtime.ensure_image('fixture:image', io.StringIO())
                if status:
                    self.assertEqual(docker.call_count, 1)
                    self.assertEqual(docker.call_args.args[0], 'build')
                    self.assertIn('Dockerfile.acceptance', str(docker.call_args.args))
                else:
                    docker.assert_not_called()

    def test_erased_image_reloads_host_archive_without_rebuilding(self):
        with TemporaryDirectory() as temporary:
            archive = Path(temporary) / 'image.tar'
            archive.write_bytes(b'retained image')
            with patch.object(runtime.subprocess, 'run', return_value=SimpleNamespace(returncode=1)), \
                    patch.object(runtime, 'docker') as docker:
                runtime.ensure_image('fixture:image', io.StringIO(), archive)
                self.assertEqual(docker.call_args.args, ('load', '-i', archive))

    def test_interruption_stops_driver_before_owned_container_cleanup(self):
        with patch.object(runtime.subprocess, 'Popen') as popen:
            process = popen.return_value
            process.wait.side_effect = [KeyboardInterrupt(), 0]
            process.poll.return_value = None
            with self.assertRaises(KeyboardInterrupt):
                runtime.run_driver(['fixture-driver'], io.StringIO())
            process.terminate.assert_called_once()
            self.assertEqual(process.wait.call_count, 2)

    def test_attempt_retains_previous_state_across_atomic_replacement(self):
        with TemporaryDirectory() as temporary:
            output = Path(temporary)
            attempt = output / 'attempts' / 'new'
            attempt.mkdir(parents=True)
            state = output / 'before.session'
            state.write_bytes(b'previous state')
            runtime.retain_previous(output, attempt)
            replacement = output / 'replacement'
            replacement.write_bytes(b'new state')
            replacement.replace(state)
            self.assertEqual((attempt / 'previous-state/before.session').read_bytes(), b'previous state')
            self.assertEqual(state.read_bytes(), b'new state')

    def test_rejection_checkpoint_preserves_negative_document(self):
        with TemporaryDirectory() as temporary:
            output = Path(temporary)
            binary = output / 'binary'
            binary.write_bytes(b'executable')
            (output / 'rejected.session').write_bytes(b'integrity-valid negative fixture')
            args = SimpleNamespace(output=output, binary=binary, scene='ISLAND', dormant='START', game_seconds=.4)
            driver = transient.TransientDriver(args)
            driver.capture = lambda: b'live public state'
            driver.checkpoint('rejected')
            self.assertEqual((output / 'rejected.session').read_bytes(), b'integrity-valid negative fixture')
            self.assertEqual((output / 'stage-rejected.session').read_bytes(), b'live public state')

    def test_checkpoint_restores_public_state_and_refuses_changed_evidence(self):
        with TemporaryDirectory() as temporary:
            output = Path(temporary)
            binary = output / 'binary'
            binary.write_bytes(b'executable')
            args = SimpleNamespace(output=output, binary=binary, scene='ISLAND', dormant='START',
                                   game_seconds=.4, runtime_output='/run/evidence')
            driver = transient.TransientDriver(args)
            driver.capture = lambda: b'public session'
            driver.checkpoint('published')
            loads = []
            driver.load = lambda *args: loads.append(args)
            driver.ready = lambda: {}
            driver.progress = lambda state: state
            self.assertEqual(driver.resume(), 'published')
            self.assertEqual(loads, [('resume-published', '/run/evidence/stage-published.session')])
            (output / 'stage-published.session').write_bytes(b'changed')
            with self.assertRaisesRegex(ValueError, 'evidence changed'):
                driver.resume()
            binary.write_bytes(b'other executable')
            with self.assertRaisesRegex(ValueError, 'executable/driver/options changed'):
                driver.resume()


if __name__ == '__main__':
    unittest.main()
