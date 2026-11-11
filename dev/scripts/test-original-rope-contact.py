#!/usr/bin/env python3
"""Public CLI controls for the original rope acceptance rig."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

DRIVER = Path(__file__).with_name('original-rope-contact.py')


class ContactDriver(unittest.TestCase):
    def run_driver(self, drawn=True, falling=0, stalled=False, fall_on_walk=False, console_opens=True, overlay=False, record=False, clock=True, acknowledge=True):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'keys').touch()
            reader = root / 'reader.py'
            placement = root / 'place.py'
            placement.write_text('pass\n')
            reader.write_text(
                'import json, pathlib, os\n'
                'keys = pathlib.Path(os.environ["CONTACT_LOG"]).read_text()\n'
                f'console = {console_opens!r} and keys.count("keyup F12\\n") > keys.count("keyup Escape\\n")\n'
                'walking = "keydown Up\\n" in pathlib.Path(os.environ["CONTACT_LOG"]).read_text()\n'
                'p=pathlib.Path(__file__).with_name("frame")\n'
                'f=int(p.read_text()) if p.exists() else 3\n'
                f'p.write_text(str(f + (0 if console else {0 if stalled else 1})))\n'
                f'print(json.dumps({{"game_seconds": f / 12 if {clock!r} else None, "clock_end": f / 12 if {clock!r} else None, "input_state": {{k: {acknowledge!r} and keys.rfind("keydown " + k + "\\n") > keys.rfind("keyup " + k + "\\n") for k in ["Up", "Control_L"]}}, "console_mode": ("overlay" if {overlay!r} else "interactive") if console else "closed", "counters": {{"mainloop_frame": f}}, "actors": '
                f'[{{"selected": True, "drawn": {drawn!r}, "falling": int(walking) if {fall_on_walk!r} else {falling}, '
                '"living": 0, "health": 100, "group": 0}]}))\n'
            )
            tool = root / 'xdotool'
            tool.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "$CONTACT_LOG"\n'
                            'if [ "$1" = search ]; then echo 1; fi\n')
            tool.chmod(0o755)
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}',
                       CONTACT_LOG=str(root / 'keys'))
            result = subprocess.run(
                [sys.executable, str(DRIVER), '--reader', str(reader),
                 '--frames', '2', '--timeout', '5', '--pulse', '.001',
                 *(['--record'] if record else []),
                 '--place', sys.executable, str(placement)],
                env=env, capture_output=True, text=True, timeout=20)
            return result, (root / 'keys').read_text()

    def test_walk_attack_and_continuation_release_every_key(self):
        result, keys = self.run_driver()
        self.assertEqual(result.returncode, 0, result.stderr)
        phases = {json.loads(line)['phase'] for line in result.stdout.splitlines()}
        self.assertTrue({'placed', 'walk-in', 'attack', 'continuation'} <= phases)
        self.assertIn('keydown Up\n', keys)
        self.assertIn('keyup Up\n', keys)
        self.assertIn('keydown Control_L\n', keys)
        self.assertTrue(keys.endswith('keyup Control_L\n'))
        self.assertNotIn('type ', keys)
        self.assertIn('keydown equal\nkeyup equal\n', keys)

    def test_recording_starts_only_after_valid_placement_and_stops(self):
        result, keys = self.run_driver(record=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(keys.count('key w\n'), 2)
        self.assertLess(keys.index('key w\n'), keys.index('keydown Up\n'))
        result, keys = self.run_driver(falling=1, record=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('key w\n', keys)

    def test_sheathed_or_falling_baseline_never_walks(self):
        for drawn, falling in [(False, 0), (True, 1)]:
            with self.subTest(drawn=drawn, falling=falling):
                result, keys = self.run_driver(drawn, falling)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('invalid contact baseline', result.stderr)
                self.assertNotIn('keydown Up\n', keys)
                self.assertNotIn('keydown Control_L\n', keys)
                if not drawn:
                    self.assertIn('keydown s\nkeyup s\n', keys)

    def test_contaminated_walk_releases_input_and_never_attacks(self):
        result, keys = self.run_driver(fall_on_walk=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('invalid contact baseline', result.stderr)
        self.assertTrue(keys.endswith('keyup Up\n'))
        self.assertNotIn('keydown Control_L', keys)

    def test_unopened_console_never_types_gameplay_bindings(self):
        result, keys = self.run_driver(console_opens=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('refusing gameplay text', result.stderr)
        self.assertNotIn('keydown m\n', keys)
        self.assertNotIn('keydown s\n', keys)
        self.assertTrue(keys.endswith('keyup Escape\n'))

    def test_paused_nonmodal_overlay_never_receives_text(self):
        result, keys = self.run_driver(overlay=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('not interactive', result.stderr)
        self.assertNotIn('keydown m\n', keys)
        self.assertNotIn('keydown s\n', keys)
        self.assertTrue(keys.endswith('keyup Escape\n'))

    def test_missing_clock_and_unacknowledged_input_never_become_accepted_boundaries(self):
        result, keys = self.run_driver(clock=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('lacks cumulative game clock', result.stderr)
        self.assertNotIn('keydown Up', keys)
        result, keys = self.run_driver(acknowledge=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('input was not acknowledged', result.stderr)
        self.assertIn('keyup Up', keys)
        self.assertNotIn('keydown Control_L', keys)

    def test_stalled_game_clock_fails_before_console(self):
        result, keys = self.run_driver(stalled=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('world clock stalled', result.stderr)
        self.assertNotIn('keydown F12', keys)


class Readiness(unittest.TestCase):
    def module(self):
        import importlib.util
        spec = importlib.util.spec_from_file_location('original_contact', DRIVER)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module

    def test_absent_actor_is_distinct_from_failed_reader_and_selection_mismatch(self):
        from unittest.mock import patch
        m = self.module()
        driver = m.Driver('reader.py', 1, .01)
        for actors, expected in [([], 'actor_absent'),
                                 ([{'selected': False}], 'selection_absent'),
                                 ([{'selected': True}, {'selected': True}], 'selection_ambiguous')]:
            result = subprocess.CompletedProcess([], 0, json.dumps({'actors': actors, 'counters': {'mainloop_frame': 5}}), '')
            with patch.object(m.subprocess, 'run', return_value=result):
                self.assertEqual(driver.observe()['readiness'], expected)
        error = subprocess.CalledProcessError(7, [], stderr='binding signature absent')
        with patch.object(m.subprocess, 'run', side_effect=error):
            row = driver.observe()
        self.assertEqual(row['readiness'], 'reader_failed')
        self.assertEqual(row['stderr'], 'binding signature absent')

    def test_loading_observations_progress_to_selected_without_input(self):
        m = self.module()
        driver = m.Driver('unused', 1, .01)
        rows = iter([dict(readiness='reader_failed', stderr='engine not loaded'),
                     dict(readiness='actor_absent', actor_count=0, frame=0, game_seconds=2),
                     dict(readiness='selected', actor_count=1, frame=3, game_seconds=4)])
        driver.observe = lambda: next(rows)
        self.assertEqual(driver.ready(2)['frame'], 3)
        self.assertEqual(driver.progress_record['polls'], 3)
        self.assertEqual(driver.progress_record['last_progress_poll'], 3)
        self.assertFalse(driver.held)

    def test_cli_progress_lock_excludes_another_attempt_before_reader_or_input(self):
        import fcntl
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            reader = root / 'reader.py'; reader.write_text('raise RuntimeError("must not run")')
            progress = root / 'progress.json'
            with progress.with_suffix('.lock').open('a') as lock:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                result = subprocess.run([sys.executable, str(DRIVER), '--reader', str(reader),
                    '--progress', str(progress), '--place', '/bin/true'], capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('BlockingIOError', result.stderr)
            self.assertFalse(progress.exists())
            self.assertEqual(result.stdout, '')

    def test_waiting_resume_preserves_diagnostics_but_running_refuses_replay(self):
        m = self.module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            reader = root / 'reader.py'; reader.write_text('pass')
            journal = root / 'progress.json'
            driver = m.Driver(str(reader), 1, .01)
            driver.journal(journal, False, ['place'])
            driver.observe = lambda: dict(readiness='actor_absent', frame=0, actor_count=0)
            with self.assertRaisesRegex(RuntimeError, 'actor_absent'):
                driver.ready(.001)
            previous = json.loads(journal.read_text())
            driver.journal(journal, True, ['place'])
            self.assertEqual(driver.progress_record['polls'], previous['polls'])
            reader.write_text('changed reader')
            with self.assertRaisesRegex(ValueError, 'source changed'):
                driver.journal(journal, True, ['place'])
            reader.write_text('pass')
            driver.mark_running()
            with self.assertRaisesRegex(ValueError, 'interrupted'):
                driver.journal(journal, True, ['place'])

class ContactLauncher(unittest.TestCase):
    def test_failed_runtime_restores_world_and_stops_only_owned_container(self):
        workspace = DRIVER.parents[2] / 'magic'
        with tempfile.TemporaryDirectory(dir=workspace) as directory:
            root = Path(directory)
            run = root / 'run'
            (run / 'Redguard').mkdir(parents=True)
            world = run / 'Redguard/WORLD.INI'
            world.write_text('[world]\nstart_world=9\n')
            system = run / 'Redguard/SYSTEM.INI'
            system.write_text('[system]\ndisable_text=no\ndisable_debug_text=no\n[debug]\nfinal_version=0\n')
            installed_system = root / 'installed-system.ini'
            installed_system.write_text('[system]\ndisable_text=yes\ndisable_debug_text=yes\n[debug]\nfinal_version=1\n')
            (run / 'game.ins').touch()
            reader = root / 'reader.py'
            reader.touch()
            placement = root / 'place.py'
            placement.touch()
            tools = root / 'tools'
            tools.mkdir()
            (tools / 'flock').write_text('#!/bin/sh\nexit 0\n')
            (tools / 'docker').write_text(
                '#!/bin/sh\nprintf "%s\\n" "$*" >> "$CONTACT_LOG"\n'
                'if [ "$1" = exec ]; then\n'
                'grep -q "disable_debug_text=yes" "$CONTACT_SYSTEM" || exit 19\n'
                'grep -q "disable_text=yes" "$CONTACT_SYSTEM" || exit 19\n'
                'grep -q "final_version=0" "$CONTACT_SYSTEM" || exit 19\n'
                'echo rejected >&2; exit 7; fi\n')
            for tool in tools.iterdir():
                tool.chmod(0o755)
            result = subprocess.run(
                ['bash', str(DRIVER.with_suffix('.sh')), str(run), str(reader),
                 str(placement), str(root / 'output')],
                env=dict(os.environ, PATH=f'{tools}:{os.environ["PATH"]}',
                         CONTACT_LOG=str(root / 'calls'),
                         CYRUS_RETRO_SYSTEM_INI=str(installed_system), CONTACT_SYSTEM=str(system)),
                capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 7, result.stderr)
            self.assertEqual(world.read_text(), '[world]\nstart_world=9\n')
            self.assertEqual(system.read_text(), '[system]\ndisable_text=no\ndisable_debug_text=no\n[debug]\nfinal_version=0\n')
            self.assertEqual((root / 'output/SYSTEM.before').read_text(), system.read_text())
            calls = (root / 'calls').read_text().splitlines()
            self.assertIn('cyrus-retro-contact:dev', calls[0])
            self.assertIn('-set dosbox hostkey=ctrlalt', calls[0])
            self.assertIn('CYRUS_CONFIG_DIR=/config', calls[0])
            self.assertIn('SYS_PTRACE', calls[0])
            name = calls[0].split('--name ')[1].split()[0]
            self.assertEqual(calls[-1], f'rm -f {name}')
            self.assertEqual((root / 'output/contact.stderr').read_text(), 'rejected\n')


if __name__ == '__main__':
    unittest.main()
