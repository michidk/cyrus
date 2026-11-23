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


class Reload(unittest.TestCase):
    def setUp(self):
        self.actor = dict(id=1, selected=True, position_q8=[100, -20, 300], yaw=64, health=96, group=8)
        self.clock = dict(nominal_scaled_total=1000, fine_scaled_total=1001,
                          next_scale=17, frame_scale=17, warmup=0)
        self.expected = dict(world=0, actors=[self.actor], clock_continuation=self.clock)
        self.ready = dict(startup=dict(menu_context=6, menu_page=2, menu_busy=False, menu_selection=1))
        self.restored = dict(world=0, actors=[dict(self.actor)], counters=dict(clock_continuation=dict(
            self.clock, nominal_scaled_total=1017, fine_scaled_total=1018)))

    def boundary(self, states):
        class Boundary:
            def __init__(self, states):
                self.states = iter(states)
                self.raw = next(self.states)
                self.calls = 0
            def raw_snapshot(self):
                return self.raw
            def next_boundary(self, timeout):
                self.calls += 1
                self.raw = next(self.states)
        return Boundary(states)

    def test_pending_context_does_not_admit_return_before_load_page_exists(self):
        for field, value in [('menu_page', 0), ('menu_selection', 0), ('menu_busy', True)]:
            raw = dict(startup=dict(self.ready['startup'], **{field: value}))
            keys = []
            with self.assertRaisesRegex(ValueError, 'Load page is not ready'):
                route.resume_checkpoint(self.boundary([raw]), keys.append, self.expected)
            self.assertEqual(keys, [])

    def test_reload_observes_menu_exit_then_first_resumed_clock_and_exact_actor(self):
        self.restored['actors'].insert(0, dict(self.actor, selected=False, group=0))
        boundary = self.boundary([self.ready, dict(world=0), self.restored])
        keys = []
        self.assertEqual(route.resume_checkpoint(boundary, keys.append, self.expected), self.restored)
        self.assertEqual(boundary.calls, 2)
        self.assertEqual(keys, [True, False, False, False])

    def test_mismatched_actor_rejects_at_first_restored_boundary_and_releases(self):
        self.restored['actors'][0]['group'] = 0
        boundary = self.boundary([self.ready, self.restored])
        keys = []
        with self.assertRaisesRegex(ValueError, 'actor 1 group expected 8, observed 0'):
            route.resume_checkpoint(boundary, keys.append, self.expected)
        self.assertEqual(boundary.calls, 1)
        self.assertFalse(keys[-1])

    def test_wrong_world_or_clock_cannot_be_accepted_as_reload(self):
        self.restored['world'] = 1
        with self.assertRaisesRegex(ValueError, 'world expected 0, observed 1'):
            route.verify_resumed_checkpoint(self.restored, self.expected)
        self.restored['world'] = 0
        self.restored['counters']['clock_continuation']['fine_scaled_total'] = 1001
        with self.assertRaisesRegex(ValueError, 'clock fine_scaled_total'):
            route.verify_resumed_checkpoint(self.restored, self.expected)

    def test_missing_reload_is_bounded_and_releases_confirm(self):
        boundary = self.boundary([self.ready, {}, {}])
        keys = []
        with self.assertRaisesRegex(ValueError, 'continuation not observed'):
            route.resume_checkpoint(boundary, keys.append, self.expected)
        self.assertEqual(boundary.calls, 2)
        self.assertFalse(keys[-1])


if __name__ == '__main__':
    unittest.main()
