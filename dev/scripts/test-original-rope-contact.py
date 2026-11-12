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

class ExactSchedule(unittest.TestCase):
    def test_exact_boundary_requires_contiguous_consumed_scales_and_input(self):
        import copy
        module = Readiness().module()
        request = dict(phase='walk-in', frames=2, key='Up')
        player = dict(drawn=True, falling=0, living=0, health=100)
        result = dict(phase='walk-in', key='Up', normal_frame_rate=12, boundaries=[
            dict(frame=10, clock_scale=1000, input_state={'Up': True}, player=player),
            dict(frame=12, clock_scale=1768, input_state={'Up': False}, player=player)], steps=[
            dict(frame=10, frame_scale=256, input_state={'Up': True}),
            dict(frame=11, frame_scale=512, input_state={'Up': True})])
        rows = module.validate_schedule(result, request)
        self.assertEqual(rows[0]['clock'], [1000 / 3072] * 2)
        self.assertEqual(rows[1]['clock'], [1768 / 3072] * 2)
        mutations = [lambda r: r['steps'].pop(),
            lambda r: r['steps'][1].update(frame=13),
            lambda r: r['steps'][1].update(frame_scale=-1),
            lambda r: r['steps'][1]['input_state'].update(Up=False),
            lambda r: r['boundaries'][1].update(clock_scale=1800),
            lambda r: r['boundaries'][1]['input_state'].update(Up=True),
            lambda r: r.update(key='Control_L'),
            lambda r: r.update(normal_frame_rate=0),
            lambda r: r['boundaries'][1]['player'].update(falling=1)]
        for mutate in mutations:
            wrong = copy.deepcopy(result); mutate(wrong)
            with self.subTest(wrong=wrong), self.assertRaises((ValueError, RuntimeError)):
                module.validate_schedule(wrong, request)

    def test_required_consumed_scale_cannot_be_replaced_by_same_sum(self):
        module = Readiness().module()
        player = dict(drawn=True, falling=0, living=0, health=100)
        result = dict(phase='walk-in', key=None, normal_frame_rate=12, boundaries=[
            dict(frame=10, clock_scale=0, input_state={}, player=player),
            dict(frame=12, clock_scale=1024, input_state={}, player=player)], steps=[
            dict(frame=10, frame_scale=256, input_state={}),
            dict(frame=11, frame_scale=768, input_state={})])
        with self.assertRaisesRegex(ValueError, 'consumed scale differs'):
            module.validate_schedule(result, dict(phase='walk-in', frames=2, key=None, frame_scale=512))

    def test_pending_interactive_request_waits_without_typing_into_closed_console(self):
        module=Readiness().module()
        from unittest.mock import patch
        driver=module.Driver('unused', 1, .01)
        pending=dict(console_mode='closed', frame=3,
                     counters=dict(console_request=1, console_requested_mode=0))
        admitted=dict(console_mode='interactive', frame=4)
        for first, accepted in [(pending, True), (dict(console_mode='overlay', frame=3), False)]:
            keys=[]
            with patch.object(driver, 'key', side_effect=keys.append), patch.object(driver, 'sample',
                    side_effect=[first, admitted, admitted]), patch.object(module.time, 'sleep'):
                if accepted: driver.command('x')
                else:
                    with self.assertRaisesRegex(RuntimeError, 'not interactive'): driver.command('x')
            self.assertEqual('x' in keys, accepted)
            self.assertEqual(keys[-1], 'Escape')

    def test_pending_console_deadline_never_types_gameplay_text(self):
        module=Readiness().module()
        from unittest.mock import patch
        driver=module.Driver('unused', 1, .01)
        pending=dict(console_mode='closed', frame=3,
                     counters=dict(console_request=1, console_requested_mode=0))
        keys=[]
        with patch.object(driver, 'key', side_effect=keys.append), patch.object(driver, 'sample',
                return_value=pending), patch.object(module.time, 'monotonic', side_effect=[0,2]):
            with self.assertRaisesRegex(RuntimeError, 'not admitted'): driver.command('x')
        self.assertEqual(keys, ['F12','Escape'])

    def test_positive_attachment_requires_unarmed_grab_hold_and_detach(self):
        import copy
        module = Readiness().module()
        for phase, flags in [('approach', [False, True]), ('hang', [True, True]), ('detach', [True, False])]:
            request = dict(phase=phase, frames=1, key=None, scenario='attachment')
            result = dict(phase=phase, key=None, normal_frame_rate=12, boundaries=[
                dict(frame=10+i, clock_scale=512*i, input_state={},
                    player=dict(drawn=False, falling=0 if flag else 2, living=0,
                                health=100, rope_attached=flag)) for i, flag in enumerate(flags)],
                steps=[dict(frame=10, frame_scale=512, input_state={})])
            self.assertEqual(len(module.validate_schedule(result, request)), 2)
            for mutation in ['weapon', 'attachment', 'missing']:
                wrong=copy.deepcopy(result)
                if mutation == 'weapon': wrong['boundaries'][0]['player']['drawn']=True
                elif mutation == 'missing': wrong['boundaries'][1]['player'].pop('rope_attached')
                else: wrong['boundaries'][1]['player']['rope_attached']=not flags[1]
                with self.subTest(phase=phase, mutation=mutation), self.assertRaises((ValueError, RuntimeError)):
                    module.validate_schedule(wrong, request)

    def test_backend_identity_change_prevents_readiness_resume(self):
        module = Readiness().module()
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            reader = root / 'reader'; reader.write_text('reader')
            backend = root / 'backend'; backend.write_text('first')
            journal = root / 'progress.json'
            driver = module.Driver(str(reader), 1, .01)
            driver.schedule_command = str(backend)
            driver.journal(journal, False, ['place'])
            backend.write_text('second')
            with self.assertRaisesRegex(ValueError, 'source changed'):
                driver.journal(journal, True, ['place'])

    def test_failed_backend_never_publishes_exact_boundaries(self):
        from unittest.mock import patch
        module = Readiness().module()
        driver = module.Driver('reader', 1, .01)
        with patch.object(module.subprocess, 'run', side_effect=subprocess.CalledProcessError(7, [])), \
                patch.object(driver, 'input') as cleanup, patch('builtins.print') as output:
            with self.assertRaises(subprocess.CalledProcessError):
                driver.exact_segment('backend', 2, 'walk-in', 'Up')
            self.assertEqual(json.loads(output.call_args.args[0])['phase'], 'schedule-failure')
            self.assertNotIn('boundary', json.loads(output.call_args.args[0]))
            cleanup.assert_called_once_with('keyup', 'Up')


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


class PhysicalRelease(unittest.TestCase):
    def test_public_cli_disables_gravity_suppression_only_after_hanging(self):
        from unittest.mock import patch
        module = Readiness().module()
        events = []
        attached = True
        class Rig:
            def __init__(self, *args): self.pulse = .001
            def ready(self, timeout): return {'counters': {'fps': 12}}
            def mark_running(self): pass
            def progress(self, *args): return {'player': {'drawn': False, 'falling': 0, 'living': 0, 'health': 100}}
            def command(self, command): events.append(command)
            def segment(self, frames, phase, key=None): events.append(phase)
            def sample(self, phase):
                events.append(phase)
                return {'player': {'drawn': False, 'falling': 0, 'living': 0, 'health': 100, 'rope_attached': attached}}
            def save(self): pass
            def release(self): events.append('released')
            progress_record = {}
        def run(physical):
            events.clear()
            with patch.object(module, 'Driver', Rig), patch.object(module.subprocess, 'check_output', return_value='1'), patch.object(module.subprocess, 'run'), patch.object(sys, 'argv', [str(DRIVER), '--reader', 'reader', '--attachment', '--schedule-command', 'backend', *(['--physical-release'] if physical else []), '--place', 'place']):
                module.main()
            return list(events)
        physical = run(True)
        self.assertLess(physical.index('hang'), physical.index('magiccarpet = 0'))
        self.assertLess(physical.index('magiccarpet = 0'), physical.index('detach'))
        self.assertIn('physical-release', physical)
        self.assertNotIn('magiccarpet = 0', run(False))
        attached = False
        with self.assertRaisesRegex(RuntimeError, 'retained attachment'):
            run(True)
        self.assertNotIn('detach', events)
        self.assertEqual(events[-1], 'released')

    def test_physical_release_rejects_non_attachment_before_input(self):
        result = subprocess.run([sys.executable, str(DRIVER), '--reader', 'absent', '--physical-release', '--place', 'absent'], capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('--physical-release requires --attachment', result.stderr)


if __name__ == '__main__':
    unittest.main()
