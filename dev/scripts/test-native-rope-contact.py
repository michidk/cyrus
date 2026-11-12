#!/usr/bin/env python3
"""Exercise accepted schedules and native failure cleanup without game data."""
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('contact', Path(__file__).with_name('native-rope-contact.py'))
contact = importlib.util.module_from_spec(spec)
spec.loader.exec_module(contact)


class Comparison(unittest.TestCase):
    def schedule(self, mutation=None):
        rows = []
        for phase in ['placed', 'walk-in', 'attack', 'continuation']:
            for frame in [10, 12]:
                rows.append(dict(phase=phase, frame=frame, boundary='start' if frame == 10 else 'end',
                    clock=[frame / 12, frame / 12], input_ack=True, player=dict(drawn=1, falling=0,
                    living=0, health=100, position=[1, 2, 3], yaw=512)))
        if mutation:
            mutation(rows)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'original.jsonl'
            path.write_text(''.join(json.dumps(row) + '\n' for row in rows))
            return contact.accepted_schedule(path)

    def test_accepted_schedule_uses_acknowledged_game_clock(self):
        player, schedule = self.schedule()
        self.assertEqual(player['yaw'], 512)
        self.assertAlmostEqual(schedule['walk-in']['duration'][0], 2 / 12)

    def test_positive_original_requires_observed_grab_hold_and_detach(self):
        rows = [dict(phase='placed', player=dict(drawn=False, falling=0, living=0, health=100,
                                               position=[1, 2, 3], yaw=512, rope_attached=False))]
        for phase, flags in [('approach', [False, True]), ('hang', [True, True]), ('detach', [True, False])]:
            for i, flag in enumerate(flags):
                rows.append(dict(phase=phase, boundary=['start','end'][i], clock=[i/2]*2,
                    input_ack=True, player=dict(drawn=False, falling=0 if flag else 2,
                        living=0, health=100, rope_attached=flag, position=[1,2,3])))
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'original.jsonl'
            def acquire():
                path.write_text(''.join(json.dumps(row)+'\n' for row in rows))
                return contact.accepted_schedule(path, attachment=True)
            _, schedule=acquire()
            self.assertEqual(set(schedule), {'approach','hang','detach'})
            rows[2]['player']['rope_attached']=False
            with self.assertRaisesRegex(ValueError, 'attachment outcome'): acquire()
            rows[2]['player']['rope_attached']=True
            rows[2]['player']['drawn']=True
            with self.assertRaisesRegex(ValueError, 'baseline'): acquire()

    def test_timing_mismatch_and_missing_clock_are_rejected(self):
        for mutation in [lambda rows: rows[2].pop('boundary'),
                         lambda rows: rows[3].update(clock=[0, 0]),
                         lambda rows: rows[2].update(input_ack=False)]:
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                self.schedule(mutation)
        with self.assertRaisesRegex(ValueError, 'timing mismatch'):
            contact.compare_interval({'duration': [1, 1]}, {'duration': [2, 2]}, .01)
        with self.assertRaisesRegex(ValueError, 'timing mismatch'):
            contact.compare_interval({'duration': [1, 2]}, {'duration': [1, 2]}, .01)
        with self.assertRaisesRegex(ValueError, 'timing mismatch'):
            contact.compare_interval({'duration': [1, 2]}, {'duration': [1.5, 1.5]}, 0)
        contact.compare_interval({'duration': [1, 1.001]}, {'duration': [1, 1.001]}, .002)

    def test_invalid_or_incomplete_original_never_drives_native(self):
        for mutation in [lambda rows: rows.__delitem__(slice(-2, None)),
                         lambda rows: rows[2]['player'].update(falling=1),
                         lambda rows: rows[3].update(clock=[0, 0]),
                         lambda rows: rows.clear()]:
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                self.schedule(mutation)

    def test_public_control_sample_records_attachment_and_world_clock(self):
        import socket
        import threading
        actor = dict(control=dict(weapon_drawn=True, falling_state=0, living_state=0,
                                  rope_attached=True), attributes=[0, 0, 0, 100],
                     spatial={'effective': {'position': [256, -512, 768]}},
                     animation={'current_group': 8})
        state = dict(scene='CATACOMB', scene_ready=True,
                     scripts={'state': 'Loaded', 'ordinary_updates': 17},
                     clock={'game_seconds': 2.5})
        with socket.socket() as server:
            server.bind(('127.0.0.1', 0))
            server.listen()
            addr = '127.0.0.1:%d' % server.getsockname()[1]
            def serve():
                for result in [state, {'actors': [actor]}, state]:
                    client, _ = server.accept()
                    with client:
                        stream = client.makefile('rb')
                        json.loads(stream.readline())
                        client.sendall((json.dumps({'ok': True, 'result': result}) + '\n').encode())
            worker = threading.Thread(target=serve, daemon=True)
            worker.start()
            output = io.StringIO()
            driver = contact.Driver(addr, 1, output)
            driver.index = 0
            row = driver.sample('walk-in')
            worker.join(2)
            self.assertFalse(worker.is_alive())
            self.assertEqual(row['frame'], 17)
            self.assertTrue(row['player']['rope_attached'])
            self.assertEqual(row['player']['position'], [1, -2, 3])
            self.assertEqual(json.loads(output.getvalue()), row)

    def test_mixer_capture_requires_recorded_pcm_and_stops_on_failure(self):
        import os
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            recorder = root / 'parec'
            recorder.write_text("#!/usr/bin/env python3\nimport signal,sys,time,wave\n"
                "w=wave.open(sys.argv[-1], 'wb');w.setparams((2,2,48000,0,'NONE',''))\n"
                "w.writeframes(bytes([1,0]) * 4800)\n"
                "def stop(*args):\n w.close();sys.exit(0)\n"
                "signal.signal(signal.SIGINT,stop)\nwhile True: time.sleep(.01)\n")
            recorder.chmod(0o755)
            from unittest.mock import patch
            output = root / 'mixer.wav'
            with patch.dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}'):
                with self.assertRaisesRegex(RuntimeError, 'gameplay failed'):
                    with contact.MixerCapture(output):
                        import time
                        time.sleep(.1)
                        raise RuntimeError('gameplay failed')
            import wave
            with wave.open(str(output)) as recorded:
                self.assertGreater(recorded.getnframes(), 0)
            output.unlink()
            recorder.write_text('#!/bin/sh\nexit 3\n')
            with patch.dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}'):
                with self.assertRaisesRegex(RuntimeError, 'mixer'):
                    with contact.MixerCapture(output):
                        time.sleep(.1)

    def scheduled(self, driver, player, clocks, end=None, mutate=None, step=1 / 12):
        calls = []
        def call(kind, **args):
            if kind == 'state':
                raise AssertionError('render FPS is not a simulation interval')
            self.assertEqual(kind, 'schedule_input')
            calls.append(args['events'])
            if len(calls) == 1:
                return {'boundaries': [dict(after_updates=i, update=10+i, generation=8,
                    game_seconds=i*step) for i in range(2)]}
            self.assertEqual(args['events'][0]['after_updates'], 0)
            rows = []
            for event, clock, value in zip(args['events'], clocks, [player, end or player]):
                actor = dict(control=dict(weapon_drawn=value['drawn'], falling_state=value['falling'],
                    living_state=value['living'], rope_attached=value.get('rope_attached')),
                    attributes=[0, 0, 0, value['health']],
                    spatial={'effective': {'position': [v * 256 for v in value['position']]}})
                rows.append(dict(event, update=20 + event['after_updates'], generation=8,
                    game_seconds=clock, player={'actors': [actor]}))
            if mutate:
                mutate(rows)
            return {'boundaries': rows}
        driver.call = call
        return calls

    def test_one_eighth_second_cannot_be_rounded_to_one_sixth(self):
        with self.assertRaisesRegex(ValueError, 'candidate common interval 0.5 seconds'):
            contact.planned_updates({'duration': [.125, .125]}, 1 / 6)
        self.assertEqual(contact.planned_updates({'duration': [.5, .5]}, 1 / 6), 3)
        self.assertEqual(contact.planned_updates({'duration': [.125, .125]}, 1 / 16), 2)

    def test_unreachable_interval_never_sends_gameplay_schedule(self):
        driver = contact.Driver('unused', 1, io.StringIO())
        calls = []
        def call(kind, **args):
            calls.append(args)
            return {'boundaries': [dict(after_updates=0, update=10, generation=8, game_seconds=0),
                dict(after_updates=1, update=11, generation=8, game_seconds=1 / 6)]}
        driver.call = call
        with self.assertRaisesRegex(ValueError, 'interval unavailable'):
            driver.timed_segment('walk-in', {'duration': [.125, .125]}, 'ArrowUp')
        self.assertEqual(len(calls), 1)
        self.assertTrue(all('key' not in event for event in calls[0]['events']))

    def test_native_positive_schedule_requires_attachment_at_exact_boundary(self):
        for attached in [True, False]:
            output=io.StringIO()
            driver=contact.Driver('unused', 1, output)
            driver.attachment=True
            start=dict(drawn=False, falling=2, living=0, health=100, position=[0,0,0], rope_attached=False)
            end=dict(start, falling=0, rope_attached=attached)
            self.scheduled(driver, start, [0,.5], end=end, step=1/6)
            expected=dict(duration=[.5,.5], start={'player': start}, end={'player': dict(end, rope_attached=True)})
            if attached:
                driver.timed_segment('approach', expected, 'Space')
                self.assertTrue(json.loads(output.getvalue().splitlines()[-1])['rope_attached'])
            else:
                with self.assertRaisesRegex(ValueError, 'attachment outcome'):
                    driver.timed_segment('approach', expected, 'Space')
                self.assertNotIn('"comparison": "matched"', output.getvalue())

    def test_unavailable_timing_probe_never_sends_gameplay_schedule(self):
        output = io.StringIO()
        driver = contact.Driver('unused', 1, output)
        for end in [dict(after_updates=1, update=12, generation=8, game_seconds=1),
                    dict(after_updates=1, update=11, generation=9, game_seconds=1),
                    dict(after_updates=1, update=11, generation=8, game_seconds=0)]:
            calls = []
            def call(kind, **args):
                calls.append(args)
                return {'boundaries': [dict(after_updates=0, update=10, generation=8, game_seconds=0), end]}
            driver.call = call
            with self.assertRaises(RuntimeError):
                driver.timed_segment('walk-in', {'duration': [1, 1]}, 'ArrowUp')
            self.assertEqual(len(calls), 1)
            self.assertTrue(all('key' not in event for event in calls[0]['events']))
            self.assertEqual(output.getvalue(), '')

    def test_attempt_length_uses_actual_simulation_step_instead_of_render_fps(self):
        output = io.StringIO()
        driver = contact.Driver('unused', 1, output)
        player = dict(drawn=True, falling=0, living=0, health=100, position=[0, 0, 0])
        calls = self.scheduled(driver, player, [0, 1], step=1 / 6)
        driver.timed_segment('walk-in', {'duration': [1, 1], 'start': {'player': player},
            'end': {'player': player}}, 'ArrowUp')
        self.assertEqual(calls[1][1]['after_updates'], 6)

    def test_wrong_update_acknowledgement_rejects_before_comparison(self):
        output = io.StringIO()
        driver = contact.Driver('unused', 1, output)
        player = dict(drawn=True, falling=0, living=0, health=100, position=[0, 0, 0])
        self.scheduled(driver, player, [0, 1], mutate=lambda rows: rows[1].update(update=99))
        with self.assertRaisesRegex(RuntimeError, 'unavailable ordinary boundaries'):
            driver.timed_segment('walk-in', {'duration': [1, 1]}, 'ArrowUp')
        self.assertNotIn('displacement', output.getvalue())

    def test_overshot_input_interval_never_emits_comparison(self):
        output = io.StringIO()
        driver = contact.Driver('unused', 1, output, .01)
        player = dict(drawn=True, falling=0, living=0, health=100, position=[0, 0, 0])
        self.scheduled(driver, player, [0, 2])
        with self.assertRaisesRegex(ValueError, 'timing mismatch'):
            driver.timed_segment('walk-in', {'duration': [1, 1]}, 'ArrowUp')
        self.assertEqual(json.loads(output.getvalue().splitlines()[-1])['comparison'], 'rejected')
        self.assertNotIn('displacement', output.getvalue())

    def test_matched_interval_reports_displacement_and_contact_only_after_gate(self):
        output = io.StringIO()
        driver = contact.Driver('unused', 1, output)
        player = dict(drawn=True, falling=0, living=0, health=100, position=[0, 0, 0], rope_attached=False)
        start = dict(clock=[0, 0], player=player)
        end = dict(clock=[1, 1], player=dict(player, position=[2, 0, 0], rope_attached=True))
        self.scheduled(driver, player, [0, 1], end['player'])
        driver.timed_segment('walk-in', contact.interval(start, end), 'ArrowUp')
        result = json.loads(output.getvalue().splitlines()[-1])
        self.assertEqual(result['comparison'], 'matched')
        self.assertEqual(result['displacement'], [2, 0, 0])
        self.assertTrue(result['rope_attached'])

    def test_scheduled_observation_interval_uses_no_gameplay_key(self):
        output = io.StringIO()
        driver = contact.Driver('unused', 1, output)
        player = dict(drawn=True, falling=0, living=0, health=100, position=[0, 0, 0])
        self.scheduled(driver, player, [0, 1])
        driver.timed_segment('continuation', {'duration': [1, 1],
            'start': {'player': player}, 'end': {'player': player}}, None)
        self.assertEqual(json.loads(output.getvalue().splitlines()[-1])['comparison'], 'matched')

    def test_readiness_timeout_resumes_without_discarding_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, output, journal = root / 'source', root / 'output', root / 'progress'
            source.write_text('accepted schedule')
            progress = contact.prepare_progress(journal, source, output, False)
            driver = contact.Driver('unused', .01, None)
            driver.call = lambda *args, **kwargs: dict(state='InGame', scene='CATACOMB',
                scene_ready=False, scripts={'state': 'Loading'})
            with self.assertRaisesRegex(RuntimeError, 'can be resumed'):
                contact.wait_ready(driver, .001, journal, progress)
            restored = contact.prepare_progress(journal, source, output, True)
            self.assertGreater(restored['polls'], 0)
            self.assertEqual(restored['last_observation']['scripts'], 'Loading')
            driver.call = lambda *args, **kwargs: dict(state='InGame', scene='CATACOMB',
                scene_ready=True, scripts={'state': 'Loaded', 'ordinary_updates': 1})
            contact.wait_ready(driver, 1, journal, restored)
            self.assertEqual(json.loads(journal.read_text())['status'], 'ready')
            self.assertFalse(output.exists())

    def test_loaded_world_without_an_update_is_not_ready(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'progress'
            progress = {'status': 'waiting', 'polls': 0}
            driver = contact.Driver('unused', .01, None)
            driver.call = lambda *args, **kwargs: dict(state='InGame', scene='CATACOMB',
                scene_ready=True, scripts={'state': 'Loaded', 'ordinary_updates': 0})
            with self.assertRaisesRegex(RuntimeError, 'timed out'):
                contact.wait_ready(driver, .001, path, progress)
            self.assertEqual(json.loads(path.read_text())['status'], 'waiting')

    def test_completed_resume_validates_output_without_sending_input(self):
        import hashlib
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, output = root / 'source', root / 'output'
            journal = output.with_suffix('.progress.json')
            source.write_text('accepted schedule')
            progress = contact.prepare_progress(journal, source, output, False)
            output.write_text('completed observations')
            progress.update(status='completed',
                            output_sha256=hashlib.sha256(output.read_bytes()).hexdigest())
            contact.save_progress(journal, progress)
            args = SimpleNamespace(output=output, original=source, resume=True)
            contact.execute_locked(args, None, None)
            output.write_text('changed observations')
            with self.assertRaisesRegex(ValueError, 'missing or changed'):
                contact.execute_locked(args, None, None)

    def test_resume_rejects_changed_source_and_interrupted_input(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source, output, journal = root / 'source', root / 'output', root / 'progress'
            source.write_text('accepted schedule')
            progress = contact.prepare_progress(journal, source, output, False)
            source.write_text('different schedule')
            with self.assertRaisesRegex(ValueError, 'source or output changed'):
                contact.prepare_progress(journal, source, output, True)
            source.write_text('accepted schedule')
            progress['status'] = 'running'
            contact.save_progress(journal, progress)
            with self.assertRaisesRegex(ValueError, 'input attempt was interrupted'):
                contact.prepare_progress(journal, source, output, True)

    def test_sword_preparation_pulses_once_instead_of_holding_across_completion(self):
        driver = contact.Driver('unused', 1, io.StringIO())
        calls = []
        def call(kind, **args):
            calls.append((kind, args))
            return args
        driver.call = call
        driver.sample = lambda phase: {'player': {'drawn': False}}
        driver.progress = lambda *args, **kwargs: None
        driver.timed_segment = lambda *args: None
        driver.run({'position': [1, 2, 3], 'yaw': 512},
                   dict.fromkeys(['walk-in', 'attack', 'continuation']))
        self.assertEqual([args for kind, args in calls if kind == 'input'],
                         [{'key': 'KeyS', 'action': 'tap'}])

    def test_rejected_native_walk_releases_held_input(self):
        driver = contact.Driver('unused', 1, io.StringIO())
        calls = []
        def call(kind, **args):
            calls.append((kind, args))
            return args
        driver.call = call
        driver.sample = lambda phase: {'player': {'drawn': True}}
        def progress(frames, phase, validate=False):
            if phase == 'walk-in':
                raise RuntimeError('native contact baseline was rejected')
        driver.progress = progress
        def segment(*args):
            driver.key('ArrowUp', True)
            raise RuntimeError('native contact baseline was rejected')
        driver.timed_segment = segment
        with self.assertRaisesRegex(RuntimeError, 'baseline was rejected'):
            driver.run({'position': [1, 2, 3], 'yaw': 512}, {'walk-in': 2})
        self.assertEqual(calls[-1], ('input', {'key': 'ArrowUp', 'action': 'release'}))
        self.assertFalse(driver.held)


class PhysicalRelease(unittest.TestCase):
    def test_original_marker_admits_gravity_only_between_hang_and_detach(self):
        import copy
        player = dict(drawn=False, falling=0, living=0, health=100, rope_attached=False, position=[1,2,3], yaw=0)
        rows = [dict(phase='placed', player=player)]
        for phase, flags in [('approach', [False, True]), ('hang', [True, True]), ('detach', [True, False])]:
            if phase == 'detach': rows.append(dict(phase='physical-release', player=dict(player, rope_attached=True)))
            for offset, flag in enumerate(flags):
                rows.append(dict(phase=phase, boundary=['start','end'][offset], clock=[offset/2]*2, input_ack=True, player=dict(player, rope_attached=flag)))
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)/'original.jsonl'
            def acquire(value):
                value = copy.deepcopy(value)
                spec = importlib.util.spec_from_file_location('original_rope', Path(__file__).with_name('original-rope-contact.py'))
                original = importlib.util.module_from_spec(spec); spec.loader.exec_module(original)
                for phase in ('approach', 'hang', 'detach'):
                    edges = [row for row in value if row.get('phase') == phase and row.get('boundary')]
                    request = dict(phase=phase, frames=1, key=None, scenario='attachment', atomic_snapshots=True)
                    boundaries = [dict(frame=i, clock_scale=i*1536, input_state={}, player=row['player']) for i,row in enumerate(edges)]
                    result = dict(phase=phase, key=None, normal_frame_rate=12, boundaries=boundaries,
                        steps=[dict(frame=0, frame_scale=1536, input_state={})],
                        snapshots=[dict(sequence=i, frame=i, clock_scale=i*1536, player=row['player'], execution='stopped',
                            boundary='ordinary_update_start', confirmation=dict(frame=i, player=row['player'])) for i,row in enumerate(edges)])
                    validated = original.validate_schedule(result, request)
                    for old,new in zip(edges,validated):old.update(new)
                    value.append(dict(phase='schedule-evidence',request=request,result=result))
                path.write_text(''.join(json.dumps(row)+'\n' for row in value))
                return contact.accepted_schedule(path, attachment=True)[1]
            self.assertTrue(acquire(rows)['detach']['physical_release'])
            accepted = [json.loads(line) for line in path.read_text().splitlines()]
            for mutation in ('missing', 'duplicate', 'pose', 'published'):
                wrong = copy.deepcopy(accepted)
                if mutation == 'missing': wrong = [row for row in wrong if row.get('phase') != 'schedule-evidence']
                elif mutation == 'published':
                    next(row for row in wrong if row.get('phase') == 'detach' and row.get('boundary') == 'end')['player']['health'] = 99
                else:
                    evidence = next(row for row in wrong if row.get('phase') == 'schedule-evidence')
                    snapshot = evidence['result']['snapshots'][1]
                    if mutation == 'duplicate': snapshot['frame'] = 0
                    else: snapshot['confirmation']['player'] = dict(snapshot['player'], health=99)
                path.write_text(''.join(json.dumps(row)+'\n' for row in wrong))
                with self.subTest(mutation=mutation), self.assertRaisesRegex(ValueError, 'snapshot'):
                    contact.accepted_schedule(path, attachment=True)
            wrong=copy.deepcopy(rows); wrong[5]['player']['rope_attached']=False
            with self.assertRaisesRegex(ValueError, 'retained attachment'): acquire(wrong)
            wrong=copy.deepcopy(rows); wrong.insert(1, wrong.pop(5))
            with self.assertRaisesRegex(ValueError, 'follow hanging'): acquire(wrong)
            wrong=copy.deepcopy(rows); wrong.insert(5, wrong[5])
            with self.assertRaisesRegex(ValueError, 'follow hanging'): acquire(wrong)

    def test_native_turns_off_fly_mode_after_hang_before_detach(self):
        driver = contact.Driver('unused', 1, io.StringIO())
        driver.attachment = True
        events=[]
        driver.call = lambda kind, **args: events.append((kind,args))
        driver.progress = lambda *args, **kwargs: None
        driver.sample = lambda phase: events.append((phase,{})) or {'player': dict(drawn=False, falling=0, living=0, health=100, rope_attached=True)}
        driver.timed_segment = lambda phase, expected, key: events.append((phase,{}))
        schedule = dict(approach={}, hang={}, detach={'physical_release': True})
        driver.run({'position':[1,2,3], 'yaw':0}, schedule)
        off=events.index(('cheat', {'name':'magiccarpet','value':0}))
        self.assertLess(events.index(('hang',{})), off)
        self.assertLess(off, events.index(('detach',{})))
        events.clear();schedule['detach']={}
        driver.run({'position':[1,2,3], 'yaw':0}, schedule)
        self.assertNotIn(('cheat', {'name':'magiccarpet','value':0}),events)
        events.clear();schedule['detach']={'physical_release': True}
        driver.sample = lambda phase: {'player': dict(drawn=False, falling=0, living=0, health=100, rope_attached=False)}
        with self.assertRaisesRegex(RuntimeError, 'retained attachment'):
            driver.run({'position':[1,2,3], 'yaw':0}, schedule)
        self.assertNotIn(('detach',{}),events)


if __name__ == '__main__':
    unittest.main()
