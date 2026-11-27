#!/usr/bin/env python3
"""Original runtime setup contracts without Docker or an installed game."""
import subprocess
import tempfile
import unittest
from pathlib import Path

import original_scenario as scenario


class OriginalRuntime(unittest.TestCase):
    def fixture(self, directory):
        magic = Path(directory)
        (magic / 'tools').mkdir()
        (magic / 'tools/original-state.py').write_text('# synthetic reader\n')
        (magic / 'gog/extracted/app').mkdir(parents=True)
        (magic / 'gog/extracted/app/game.gog').write_bytes(b'synthetic disc')
        run = magic / 'run'
        (run / 'Redguard/maps').mkdir(parents=True)
        return magic, run

    def test_start_uses_owned_container_mounts_and_both_reader_files(self):
        with tempfile.TemporaryDirectory() as directory:
            magic, run = self.fixture(directory)
            calls = []
            scenario.start_original_runtime('owned', run, 'retained:image', magic=magic,
                command=lambda *args, **kwargs: calls.append((args, kwargs)))
            self.assertEqual(calls[0], (('docker', 'rm', '-f', 'owned'), {'check': False}))
            prepare, launch = calls[1:3]
            self.assertIn('/game/Redguard/maps', prepare[0])
            self.assertEqual(prepare[0][-3:], ('-iname', '*.tsg', '-delete'))
            self.assertIn(f'{run}:/game', launch[0])
            self.assertIn(f'{magic}/gog:/gog:ro', launch[0])
            self.assertEqual((run / 'game.gog').resolve(), magic / 'gog/extracted/app/game.gog')
            self.assertFalse(any('/game/game.gog:ro' in arg for arg in launch[0]))
            self.assertEqual(launch[0][-1], 'retained:image')
            self.assertEqual([call[0][-1] for call in calls[3:]],
                ['owned:/tmp/original-state.py', 'owned:/tmp/original-memory-region.py',
                 'owned:/tmp/original_script_locals.py', 'owned:/tmp/original_source_state.py'])
            self.assertTrue(all(kwargs['check'] for _, kwargs in calls[1:]))

    def test_disc_preparation_replaces_only_empty_mountpoint_and_shares_canonical_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            magic, run = self.fixture(directory)
            target = run / 'game.gog'
            target.touch()
            scenario.prepare_original_disc(run, magic=magic)
            self.assertTrue(target.is_symlink())
            self.assertEqual(target.read_bytes(), b'synthetic disc')
            scenario.prepare_original_disc(run, magic=magic)
            target.unlink()
            target.write_bytes(b'private media')
            with self.assertRaisesRegex(ValueError, 'existing.*game.gog'):
                scenario.prepare_original_disc(run, magic=magic)
            self.assertEqual(target.read_bytes(), b'private media')
            self.assertEqual((magic / 'gog/extracted/app/game.gog').read_bytes(), b'synthetic disc')

    def test_missing_reader_fails_before_docker(self):
        with tempfile.TemporaryDirectory() as directory:
            calls = []
            with self.assertRaises(FileNotFoundError):
                scenario.start_original_runtime('owned', Path(directory), 'image', magic=directory,
                    command=lambda *args, **kwargs: calls.append(args))
            self.assertEqual(calls, [])

    def test_shared_maps_rejected_before_cleanup(self):
        with tempfile.TemporaryDirectory() as directory:
            magic, run = self.fixture(directory)
            maps = run / 'Redguard/maps'
            maps.rmdir()
            maps.symlink_to(magic / 'tools', target_is_directory=True)
            calls = []
            with self.assertRaisesRegex(ValueError, 'run-local writable maps'):
                scenario.start_original_runtime('owned', run, 'image', magic=magic,
                    command=lambda *args, **kwargs: calls.append(args))
            self.assertEqual(calls, [])

    def test_failed_copy_cleans_only_owned_container_and_keeps_host_evidence(self):
        with tempfile.TemporaryDirectory() as directory:
            magic, run = self.fixture(directory)
            evidence = run / 'completed.jsonl'
            evidence.write_text('retained')
            calls = []
            def command(*args, **kwargs):
                calls.append(args)
                if args[:2] == ('docker', 'cp'):
                    raise subprocess.CalledProcessError(1, args)
            with self.assertRaises(subprocess.CalledProcessError):
                scenario.start_original_runtime('owned', run, 'image', magic=magic, command=command)
            self.assertEqual(calls[-1], ('docker', 'rm', '-f', 'owned'))
            self.assertEqual(evidence.read_text(), 'retained')

    def test_combat_bundle_imports_shared_api_outside_script_directory(self):
        import os
        import shutil
        with tempfile.TemporaryDirectory() as directory:
            destination = Path(directory) / 'bundle'
            def command(*args, **kwargs):
                if args[:2] == ('docker', 'exec'):
                    destination.mkdir()
                else:
                    shutil.copy2(args[2], destination / Path(args[2]).name)
            installed = scenario.install_original_combat_driver('owned', command=command)
            self.assertEqual(installed, '/tmp/cyrus-combat')
            result = subprocess.run(['python3', '-c',
                'import original_fight, original_acquisition, original_sky_state; '
                'assert callable(original_fight.record_combat_tap); '
                'assert callable(original_acquisition.OriginalAcquisition); '
                'assert callable(original_sky_state.read_sky_state)'],
                cwd=directory, env=dict(os.environ, PYTHONPATH=str(destination)),
                capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertFalse((destination / 'original-fight.py').exists())

    def test_attach_only_installs_reader_dependencies(self):
        with tempfile.TemporaryDirectory() as directory:
            magic, _ = self.fixture(directory)
            calls = []
            scenario.install_original_reader('attached', magic=magic,
                command=lambda *args, **kwargs: calls.append(args))
            self.assertEqual(len(calls), 4)
            self.assertTrue(all(call[:2] == ('docker', 'cp') for call in calls))


if __name__ == '__main__':
    unittest.main()
