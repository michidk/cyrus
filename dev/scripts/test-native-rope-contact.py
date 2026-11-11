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

    def test_overshot_input_interval_never_emits_comparison(self):
        output = io.StringIO()
        driver = contact.Driver('unused', 1, output, .01)
        player = dict(drawn=True, falling=0, living=0, health=100, position=[0, 0, 0])
        boundaries = iter([dict(clock=[0, 0], player=player), dict(clock=[2, 2], player=player)])
        driver.boundary = lambda *args: next(boundaries)
        driver.sample = lambda phase: dict(game_seconds=2, player=player)
        with self.assertRaisesRegex(ValueError, 'timing mismatch'):
            driver.timed_segment('walk-in', {'duration': [1, 1]}, 'ArrowUp')
        self.assertEqual(json.loads(output.getvalue())['comparison'], 'rejected')
        self.assertNotIn('displacement', output.getvalue())

    def test_matched_interval_reports_displacement_and_contact_only_after_gate(self):
        output = io.StringIO()
        driver = contact.Driver('unused', 1, output)
        player = dict(drawn=True, falling=0, living=0, health=100, position=[0, 0, 0], rope_attached=False)
        start = dict(clock=[0, 0], player=player)
        end = dict(clock=[1, 1], player=dict(player, position=[2, 0, 0], rope_attached=True))
        boundaries = iter([start, end])
        driver.boundary = lambda *args: next(boundaries)
        driver.sample = lambda phase: dict(game_seconds=1, player=player)
        driver.timed_segment('walk-in', contact.interval(start, end), 'ArrowUp')
        result = json.loads(output.getvalue())
        self.assertEqual(result['comparison'], 'matched')
        self.assertEqual(result['displacement'], [2, 0, 0])
        self.assertTrue(result['rope_attached'])

    def test_input_boundary_waits_for_player_acknowledgement(self):
        driver = contact.Driver('unused', 1, io.StringIO())
        samples = iter([dict(game_seconds=1, clock_end=1, input_state={'forward': False}, player={}),
                        dict(game_seconds=1.1, clock_end=1.1, input_state={'forward': False}, player={}),
                        dict(game_seconds=1.2, clock_end=1.2, input_state={'forward': True}, player={})])
        driver.sample = lambda phase: next(samples)
        driver.call = lambda kind, **args: args
        row = driver.boundary('walk-in', 'start', 'ArrowUp')
        self.assertEqual(row['clock'], [1, 1.2])
        self.assertTrue(row['input_ack'])
        driver.call = lambda *args, **kwargs: {}
        with self.assertRaisesRegex(RuntimeError, 'acknowledgement'):
            driver.key('ArrowUp', False)

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


if __name__ == '__main__':
    unittest.main()
