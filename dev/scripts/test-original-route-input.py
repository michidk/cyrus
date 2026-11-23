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


class ScriptedItem(unittest.TestCase):
    def state(self, pending=False, item=18, frame=10, world=1):
        return dict(world=world, counters=dict(mainloop_frame=frame,
                    item_view=dict(pending=pending, item=item)))

    def boundary(self, states):
        class Boundary:
            stopped = True
            def __init__(self):
                self.states = iter(states)
                self.raw = next(self.states)
                self.calls = []
            def raw_snapshot(self):
                return self.raw
            def select_boundary(self, binding):
                self.calls.append(('binding', binding))
            def next_boundary(self, timeout):
                self.raw = next(self.states)
            def latch_key(self, key, action):
                self.calls.append((key, action))
        return Boundary()

    def test_item_request_modal_updates_and_ordinary_resume_are_distinct(self):
        pending = self.state(True)
        end = self.state(frame=11)
        b = self.boundary([self.state(), pending, pending, pending, pending, end])
        self.assertEqual(route.dismiss_scripted_item(b, 1, 18, 16, 24, 32,
                                                    opening_updates=2), end)
        self.assertEqual(b.calls, [('binding', 16), ('binding', 24),
                         ('Escape', 'press'), ('Escape', 'release'),
                         ('binding', 32), ('binding', 32)])

    def test_request_flag_can_precede_its_item_assignment_but_clock_boundary_cannot(self):
        pending = self.state(True)
        end = self.state(frame=11)
        b = self.boundary([self.state(), self.state(True, item=0), pending, pending, end])
        self.assertEqual(route.dismiss_scripted_item(b, 1, 18, 16, 24, 32,
                                                    opening_updates=1), end)

    def test_bounded_request_arrival_establishes_its_own_modal_frame(self):
        pending = self.state(True, frame=11)
        end = self.state(frame=12)
        b = self.boundary([self.state(), pending, pending, pending, end])
        self.assertEqual(route.dismiss_scripted_item(b, 1, 18, 16, 24, 32,
                         opening_updates=1, request_updates=1), end)

    def test_wrong_item_is_not_dismissed(self):
        b = self.boundary([self.state(), self.state(True, item=19), self.state(True, item=19)])
        with self.assertRaisesRegex(ValueError, 'expected pending item'):
            route.dismiss_scripted_item(b, 1, 18, 16, 24, 32, opening_updates=2)
        self.assertNotIn(('Escape', 'press'), b.calls)
        self.assertEqual(b.calls[-1], ('binding', 32))

    def test_failure_after_press_releases_input_and_restores_ordinary_binding(self):
        pending = self.state(True)
        b = self.boundary([self.state(), pending, pending])
        with self.assertRaises(StopIteration):
            route.dismiss_scripted_item(b, 1, 18, 16, 24, 32, opening_updates=1)
        self.assertEqual(b.calls[-2:], [('Escape', 'release'), ('binding', 32)])

    def test_timeout_while_held_pauses_before_releasing_the_key(self):
        pending = self.state(True)
        b = self.boundary([pending, pending])
        advance = b.next_boundary
        def timeout_after_opening(timeout):
            if ('Escape', 'press') in b.calls:
                b.stopped = False
                raise TimeoutError('modal clock unavailable')
            advance(timeout)
        def pause():
            b.calls.append(('pause',))
            b.stopped = True
        b.next_boundary, b.pause = timeout_after_opening, pause
        with self.assertRaisesRegex(TimeoutError, 'modal clock unavailable'):
            route.dismiss_scripted_item(b, 1, 18, 16, 24, 32, opening_updates=1)
        self.assertEqual(b.calls[-3:], [('pause',), ('Escape', 'release'), ('binding', 32)])

    def test_world_or_frame_change_during_modal_is_not_ordinary_readiness(self):
        for invalid in [self.state(True, frame=11), self.state(True, world=2)]:
            b = self.boundary([self.state(), invalid])
            with self.assertRaisesRegex(ValueError, 'expected pending item|request exceeded'):
                route.dismiss_scripted_item(b, 1, 18, 16, 24, 32, opening_updates=1)
        pending = self.state(True)
        b = self.boundary([self.state(), pending, pending, pending, self.state(frame=12)])
        with self.assertRaisesRegex(ValueError, 'resumption not acknowledged'):
            route.dismiss_scripted_item(b, 1, 18, 16, 24, 32, opening_updates=1)


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

    def restored_boundary(self, states):
        boundary = self.boundary(states)
        boundary.stopped = True
        boundary.bindings = []
        boundary.select_boundary = boundary.bindings.append
        return boundary

    def test_loader_boundary_preserves_moving_saved_state_until_explicit_update(self):
        saved = dict(world=0, actors=[dict(self.actor)],
                     counters=dict(clock_continuation=dict(self.clock)))
        moved = dict(world=0, actors=[dict(self.actor, position_q8=[101, -20, 302])],
                     counters=self.restored['counters'])
        boundary = self.restored_boundary([self.ready, dict(world=0), saved, moved])
        keys = []
        result = route.restore_checkpoint(boundary, keys.append, self.expected, [(16, 2)], 32)
        self.assertEqual(result, saved)
        self.assertEqual(boundary.calls, 2)
        self.assertEqual(boundary.bindings, [16, 32])
        self.assertEqual(keys, [True, False, False, False])
        boundary.next_boundary(1)
        self.assertEqual(boundary.raw_snapshot(), moved)
        with self.assertRaisesRegex(ValueError, 'position_q8'):
            route.verify_resumed_checkpoint(moved, self.expected, restored=True)

    def test_restored_clocks_with_absent_actors_do_not_identify_loader_completion(self):
        partial = dict(world=0, actors=[], counters=dict(clock_continuation=dict(self.clock)))
        saved = dict(partial, actors=[dict(self.actor)])
        boundary = self.restored_boundary([self.ready, partial, saved])
        self.assertEqual(route.restore_checkpoint(boundary, lambda _: None, self.expected,
                                                 [(16, 1), (24, 1)], 32), saved)
        self.assertEqual(boundary.bindings, [16, 24, 32])
        boundary = self.restored_boundary([self.ready, partial])
        with self.assertRaisesRegex(ValueError, 'actor 1 missing'):
            route.restore_checkpoint(boundary, lambda _: None, self.expected,
                                     [(16, 1)], 32)

    def test_exact_player_does_not_hide_a_post_update_nonplayer_pose(self):
        npc = dict(self.actor, id=2, selected=False, position_q8=[400, -20, 500], group=167)
        expected = dict(self.expected, actors=[self.actor, npc])
        raw = dict(world=0, actors=[dict(self.actor), dict(npc, yaw=65)],
                   counters=dict(clock_continuation=dict(self.clock)))
        with self.assertRaisesRegex(ValueError, 'actor 2 yaw expected 64, observed 65'):
            route.verify_resumed_checkpoint(raw, expected, restored=True)

    def test_post_update_clock_is_not_accepted_as_restored_state(self):
        boundary = self.restored_boundary([self.ready, self.restored])
        keys = []
        with self.assertRaisesRegex(ValueError, 'nominal_scaled_total expected 1000, observed 1017'):
            route.restore_checkpoint(boundary, keys.append, self.expected, [(16, 1)], 32)
        self.assertEqual(boundary.bindings, [16, 32])
        self.assertFalse(keys[-1])

    def test_restore_checks_fractional_clock_fields_and_rejects_invalid_budget_before_input(self):
        saved = dict(world=0, actors=[dict(self.actor)],
                     counters=dict(clock_continuation=dict(self.clock, fine_ticks=3)))
        expected = dict(self.expected, clock_continuation=dict(self.clock, fine_ticks=4))
        boundary = self.restored_boundary([self.ready, saved])
        with self.assertRaisesRegex(ValueError, 'fine_ticks expected 4, observed 3'):
            route.restore_checkpoint(boundary, lambda _: None, expected, [(16, 1)], 32)
        boundary = self.restored_boundary([self.ready])
        keys = []
        with self.assertRaisesRegex(ValueError, 'positive write-count sequence'):
            route.restore_checkpoint(boundary, keys.append, expected, [(16, 0)], 32)
        self.assertEqual(keys, [])
        self.assertEqual(boundary.bindings, [])

    def test_partial_restore_rejects_exact_actors_without_advancing_further(self):
        partial = dict(world=0, actors=[dict(self.actor, group=0)],
                       counters=dict(clock_continuation=dict(self.clock)))
        boundary = self.restored_boundary([self.ready, partial])
        with self.assertRaisesRegex(ValueError, 'group expected 8, observed 0'):
            route.restore_checkpoint(boundary, lambda _: None, self.expected, [(16, 1)], 32)
        self.assertEqual(boundary.calls, 1)
        self.assertEqual(boundary.bindings, [16, 32])

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


class CheckpointedPhase(unittest.TestCase):
    def test_acknowledged_attacks_without_damage_fail_instead_of_advancing_route(self):
        session = Session()
        with self.assertRaisesRegex(RuntimeError, 'opening damage.*checkpoint.*3 updates'):
            route.play_checkpointed_phase(session, 'opening damage', lambda state: {'ControlLeft'},
                                          lambda state: state['health'] < 100, 3)
        self.assertEqual(session.frame, 43)
        self.assertEqual(session.calls[-1], ('ControlLeft', 'release', 43))

    def test_progress_stops_on_first_observed_checkpoint_and_resumes_without_extra_input(self):
        session = Session()
        def snapshot():
            return dict(frame=session.frame, world=session.world, health=90 if session.frame >= 42 else 100)
        session.snapshot = snapshot
        complete = lambda state: state['health'] < 100
        receipt = route.play_checkpointed_phase(session, 'damage', lambda state: {'ControlLeft'}, complete, 5)
        self.assertEqual(receipt['end']['frame'], 42)
        self.assertEqual(receipt['checkpoint'], 'damage')
        self.assertEqual(receipt['inputs'][0]['acknowledged'], 40)
        session.calls.clear()
        route.play_checkpointed_phase(session, 'damage', lambda state: {'ControlLeft'}, complete, 5)
        self.assertEqual(session.calls, [])

    def test_world_change_between_input_events_rejects_before_checkpoint(self):
        session = Session()
        def advance(frame):
            session.frame = frame
            session.world = 1
        session.advance_to = advance
        with self.assertRaisesRegex(RuntimeError, 'world changed.*40.*41'):
            route.play_checkpointed_phase(session, 'combat', lambda state: {'ControlLeft'},
                                          lambda state: state['world'] == 1, 3)
        self.assertEqual(session.calls[-1], ('ControlLeft', 'release', 41))

    def test_missed_key_acknowledgement_releases_and_stops_before_advancing(self):
        session = Session()
        session.bad_ack = True
        with self.assertRaisesRegex(RuntimeError, 'expected frame 40, acknowledged 41'):
            route.play_checkpointed_phase(session, 'combat', lambda state: {'ControlLeft'}, lambda state: False, 3)
        self.assertEqual(session.frame, 40)
        self.assertEqual(session.calls[-1], ('ControlLeft', 'release', 40))

    def test_policy_failure_releases_previously_held_keys(self):
        session = Session()
        def policy(state):
            if state['frame'] > 40:
                raise ValueError('missing opponent state')
            return {'ControlLeft'}
        with self.assertRaisesRegex(ValueError, 'missing opponent state'):
            route.play_checkpointed_phase(session, 'combat', policy, lambda state: False, 3)
        self.assertEqual(session.calls[-1], ('ControlLeft', 'release', 41))

    def test_invalid_budget_and_no_world_reject_before_input(self):
        session = Session()
        with self.assertRaisesRegex(ValueError, 'budget'):
            route.play_checkpointed_phase(session, 'combat', lambda state: set(), lambda state: False, -1)
        session.world = 4294967295
        with self.assertRaisesRegex(ValueError, 'running world'):
            route.play_checkpointed_phase(session, 'combat', lambda state: set(), lambda state: False, 3)
        self.assertEqual(session.calls, [])

    def test_aim_releases_spin_before_attack_and_does_not_attack_outside_authored_range(self):
        state = dict(actors=[dict(id=1, selected=True, position_q8=[0, 0, 0], yaw=0),
                             dict(id=2, selected=False, position_q8=[-25600, 0, 0], yaw=0)])
        attack = dict(arc=128, min_range=0, max_range=35000)
        self.assertEqual(route.aimed_combat_keys(state, 2, attack, {'ControlLeft'}), {'ArrowLeft'})
        state['actors'][0]['yaw'] = 512
        self.assertEqual(route.aimed_combat_keys(state, 2, attack, {'ControlLeft'}), {'ControlLeft'})
        state['actors'][1]['position_q8'][0] = -45000
        self.assertEqual(route.aimed_combat_keys(state, 2, attack, {'ControlLeft'}), {'ArrowUp'})
        state['actors'][1]['position_q8'] = [0, 0, 0]
        self.assertEqual(route.aimed_combat_keys(state, 2, attack, {'ControlLeft'}), set())


class WalkRoute(unittest.TestCase):
    def setUp(self):
        self.point = dict(name='street', position_q8=[0, 0, 30], radius_q8=2,
                          height_q8=5, modifier=True, max_updates=5)
        self.session = Session()
        self.position = [0, 0, 0]
        self.lockout = 0
        self.session.snapshot = lambda: dict(frame=self.session.frame, world=self.session.world,
            actors=[dict(selected=True, position_q8=list(self.position), yaw=0, lockout=self.lockout)])

    def test_completion_requires_pose_and_control_not_acknowledged_forward_input(self):
        with self.assertRaisesRegex(RuntimeError, 'street checkpoint not reached'):
            route.play_walk_route(self.session, [self.point], turn_window=48)
        self.assertEqual(self.session.calls[-2:], [('ArrowUp', 'release', 45), ('ShiftLeft', 'release', 45)])

    def test_measured_waypoint_releases_and_resume_skips_only_explicit_completed_legs(self):
        def advance(frame):
            self.session.frame = frame
            self.position[2] += 10
        self.session.advance_to = advance
        result = route.play_walk_route(self.session, [self.point], turn_window=48)
        self.assertEqual(result[0]['next_index'], 1)
        self.assertEqual(result[0]['end']['frame'], 43)
        self.session.calls.clear()
        self.assertEqual(route.play_walk_route(self.session, [self.point], resume=1, turn_window=48), [])
        self.assertEqual(self.session.calls, [])

    def test_turning_precedes_forward_and_height_cannot_complete_horizontal_disk(self):
        state = self.session.snapshot()
        state['actors'][0]['yaw'] = 512
        self.assertEqual(route.waypoint_keys(state, self.point, 48), {'ArrowRight'})
        self.position[:] = [0, 10, 30]
        self.assertFalse(route.waypoint_reached(self.session.snapshot(), self.point))
        self.lockout = 1
        with self.assertRaisesRegex(ValueError, 'control is locked'):
            route.play_walk_route(self.session, [self.point], turn_window=48)

    def test_completed_receipt_survives_later_stall_and_can_resume_at_that_leg(self):
        self.position[:] = self.point['position_q8']
        completed = []
        later = dict(self.point, name='gate', position_q8=[0, 0, 60], max_updates=1)
        with self.assertRaisesRegex(RuntimeError, 'gate checkpoint not reached'):
            route.play_walk_route(self.session, [self.point, later], turn_window=48,
                                  checkpoint=completed.append)
        self.assertEqual([r['next_index'] for r in completed], [1])
        self.position[:] = later['position_q8']
        receipts = route.play_walk_route(self.session, [self.point, later],
                                        resume=completed[-1]['next_index'], turn_window=48)
        self.assertEqual([r['checkpoint'] for r in receipts], ['gate'])
        self.assertEqual(receipts[0]['next_index'], 2)

    def test_bad_later_waypoint_rejects_before_first_input(self):
        with self.assertRaisesRegex(ValueError, 'invalid waypoint'):
            route.play_walk_route(self.session, [self.point, dict(self.point, radius_q8=-1)], turn_window=48)
        self.assertEqual(self.session.calls, [])


if __name__ == '__main__':
    unittest.main()
