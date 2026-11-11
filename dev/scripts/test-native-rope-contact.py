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
                rows.append(dict(phase=phase, frame=frame, player=dict(drawn=1, falling=0,
                    living=0, health=100, position=[1, 2, 3], yaw=512)))
        if mutation:
            mutation(rows)
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'original.jsonl'
            path.write_text(''.join(json.dumps(row) + '\n' for row in rows))
            return contact.accepted_schedule(path)

    def test_accepted_schedule_uses_observed_updates(self):
        player, schedule = self.schedule()
        self.assertEqual(player['yaw'], 512)
        self.assertEqual(schedule, {'walk-in': 2, 'attack': 2, 'continuation': 2})

    def test_invalid_or_incomplete_original_never_drives_native(self):
        for mutation in [lambda rows: rows.__delitem__(slice(-2, None)),
                         lambda rows: rows[2]['player'].update(falling=1),
                         lambda rows: rows[3].update(frame=9),
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
                for result in [state, {'actors': [actor]}]:
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

    def test_rejected_native_walk_releases_held_input(self):
        driver = contact.Driver('unused', 1, io.StringIO())
        calls = []
        driver.call = lambda kind, **args: calls.append((kind, args))
        driver.sample = lambda phase: {'player': {'drawn': True}}
        def progress(frames, phase, validate=False):
            if phase == 'walk-in':
                raise RuntimeError('native contact baseline was rejected')
        driver.progress = progress
        with self.assertRaisesRegex(RuntimeError, 'baseline was rejected'):
            driver.run({'position': [1, 2, 3], 'yaw': 512}, {'walk-in': 2})
        self.assertEqual(calls[-1], ('input', {'key': 'ArrowUp', 'action': 'release'}))
        self.assertFalse(driver.held)


if __name__ == '__main__':
    unittest.main()
