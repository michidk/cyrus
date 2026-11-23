#!/usr/bin/env python3
"""Original route input is relative, bounded and released even on rejection."""
import importlib.util
import json
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('route', Path(__file__).with_name('original-route-input.py'))
route = importlib.util.module_from_spec(spec)
spec.loader.exec_module(route)


class Session:
    def __init__(self):
        self.frame, self.world = 40, 0
        self.calls = []
        self.bad_ack = False

    def snapshot(self):
        return dict(frame=self.frame, world=self.world, health=100)

    def advance_to(self, frame):
        self.calls.append(('advance', frame))
        self.frame = frame

    def input(self, key, action):
        self.calls.append((key, action, self.frame))
        return self.frame + int(self.bad_ack)


class RouteInput(unittest.TestCase):
    def test_fixture_offsets_are_relative_to_current_boundary_and_waits_are_retained(self):
        session = Session()
        result = route.play_input_block(session, [
            dict(after_updates=0, key='ControlLeft', action='press'),
            dict(after_updates=2, key='ControlLeft', action='release'), dict(after_updates=5)], 5)
        self.assertEqual(session.calls, [('advance', 40), ('ControlLeft', 'press', 40),
                                       ('advance', 42), ('ControlLeft', 'release', 42), ('advance', 45)])
        self.assertEqual(result['end']['frame'], 45)
        self.assertEqual([item['acknowledged'] for item in result['inputs']], [40, 42])

    def test_checked_in_opening_fixture_combat_block_reaches_its_last_input_boundary(self):
        fixture = json.loads((Path(__file__).parents[1] / 'fixtures/chapters/000-opening.replay.json').read_text())
        events = fixture['steps'][1]['repeat']['steps'][1]['input']
        session = Session()
        result = route.play_input_block(session, events, 100)
        self.assertEqual(result['end']['frame'], 140)
        self.assertEqual(result['inputs'][-1], dict(frame=140, key='ArrowRight', action='release', acknowledged=140))
        self.assertFalse(any(key == 'KeyH' for key, *_ in session.calls))

    def test_invalid_later_event_rejects_before_any_input(self):
        session = Session()
        with self.assertRaisesRegex(ValueError, 'ordered and bounded'):
            route.play_input_block(session, [dict(after_updates=0, key='ControlLeft', action='press'),
                                            dict(after_updates=6)], 5)
        self.assertEqual(session.calls, [])

    def test_negative_offset_and_no_world_reject_before_input(self):
        session = Session()
        with self.assertRaisesRegex(ValueError, 'ordered and bounded'):
            route.play_input_block(session, [dict(after_updates=-1)], 5)
        session.world = 4294967295
        with self.assertRaisesRegex(ValueError, 'running world boundary'):
            route.play_input_block(session, [dict(after_updates=0, key='ControlLeft', action='press')], 5)
        self.assertEqual(session.calls, [])

    def test_bad_acknowledgement_releases_a_possibly_applied_press(self):
        session = Session()
        session.bad_ack = True
        with self.assertRaisesRegex(RuntimeError, 'expected frame 40, acknowledged 41'):
            route.play_input_block(session, [dict(after_updates=0, key='ControlLeft', action='press')], 1)
        self.assertEqual(session.calls[-1], ('ControlLeft', 'release', 40))

    def test_world_change_rejects_and_releases_without_sending_next_input(self):
        session = Session()
        def advance(frame):
            session.frame = frame
            if frame > 40:
                session.world = 1
        session.advance_to = advance
        with self.assertRaisesRegex(RuntimeError, 'world changed'):
            route.play_input_block(session, [dict(after_updates=0, key='ArrowRight', action='press'),
                                            dict(after_updates=1, key='ControlLeft', action='press')], 1)
        self.assertEqual(session.calls, [('ArrowRight', 'press', 40), ('ArrowRight', 'release', 41)])


if __name__ == '__main__':
    unittest.main()
