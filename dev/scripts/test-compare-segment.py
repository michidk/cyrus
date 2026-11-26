#!/usr/bin/env python3
"""Exercise paired execution, wrong-version detection and invalid timing admission."""
from contextlib import contextmanager
import copy
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('pair', Path(__file__).with_name('compare-segment.py'))
pair = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pair)

SEGMENT = {'steps': [{'input': [{'after_updates': 0, 'key': 'ControlLeft', 'action': 'press'},
                              {'after_updates': 2, 'key': 'ControlLeft', 'action': 'release'}]},
                     {'checkpoint': {'flags': {'talking': {'value': 1}}}}]}


class Session:
    def __init__(self, states, calls, name, overshoot=False):
        self.states, self.calls, self.name, self.overshoot = states, calls, name, overshoot
        self.frame = 0

    def snapshot(self):
        return dict(frame=self.frame, **copy.deepcopy(self.states.get(self.frame, self.states[0])))

    def advance_to(self, frame):
        self.frame = frame + int(self.overshoot)

    def input(self, key, action):
        self.calls.append((self.name, self.frame, key, action))
        return self.frame


def state(x=0, talking=0):
    return dict(world=20, player=dict(position_q8=[x, -16384, 587520], heading=0, health=100,
                                     animation_group=0, control=0),
                actors={'FAVIS': {'position_q8': [545024, -15872, 613120], 'health': 100}},
                flags={'talking': talking}, prompts=[], timing={'elapsed_scale': 0},
                continuations={'FAVIS': {'cursor': 0, 'tasks': []}})


class PairTests(unittest.TestCase):
    def run_pair(self, original, native, overshoot=False):
        self.calls = []
        def factory(name, states):
            @contextmanager
            def open_session(segment, output):
                self.calls.append(('open', name))
                try:
                    yield Session(states, self.calls, name, overshoot and name == 'original')
                finally:
                    self.calls.append(('close', name))
            return open_session
        with tempfile.TemporaryDirectory() as directory:
            return pair.run_pair(SEGMENT, factory('original', original), factory('native', native), Path(directory))

    def test_original_finishes_before_native_and_both_receive_identical_frame_inputs(self):
        report = self.run_pair({0: state(), 3: state(talking=1)}, {0: state(), 3: state(talking=1)})
        self.assertEqual(report['status'], 'match')
        self.assertLess(self.calls.index(('close', 'original')), self.calls.index(('open', 'native')))
        self.assertEqual([row[1:] for row in self.calls if row[0] == 'original'],
                         [(0, 'ControlLeft', 'press'), (2, 'ControlLeft', 'release')])
        self.assertEqual([row[1:] for row in self.calls if row[0] == 'original'],
                         [row[1:] for row in self.calls if row[0] == 'native'])

    def test_wrong_flag_and_position_produce_named_deltas_at_the_checkpoint(self):
        report = self.run_pair({0: state(), 3: state(5, 1)}, {0: state(), 3: state(9, 0)})
        self.assertEqual(report['status'], 'different')
        fields = {row['field']: row for row in report['checkpoints'][-1]['fields']}
        self.assertEqual(fields['flags.talking']['delta'], -1)
        self.assertEqual(fields['player.position_q8[0]']['delta'], 4)
        self.assertEqual(fields['flags.talking']['original'], 1)

    def test_first_different_checkpoint_stops_native_before_later_inputs(self):
        segment = copy.deepcopy(SEGMENT)
        segment['steps'] += [{'input': [{'after_updates': 0, 'key': 'ArrowDown',
                                        'action': 'press'}]}]
        self.calls = []
        @contextmanager
        def factory(name, values):
            yield Session(values, self.calls, name)
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(segment, lambda *_: factory('original', {0: state()}),
                                   lambda *_: factory('native', {0: state(), 3: state(1)}),
                                   Path(directory))
        self.assertEqual(report['status'], 'different')
        self.assertEqual(report['checkpoints'][-1]['frame'], 3)
        self.assertEqual([row['frame'] for row in report['native']], [0, 3])
        self.assertNotIn(('native', 3, 'ArrowDown', 'press'), self.calls)

    def test_completed_boundaries_survive_a_later_backend_failure(self):
        @contextmanager
        def failing(segment, output):
            class Failing(Session):
                def advance_to(self, frame):
                    raise TimeoutError('ordinary boundary stalled')
            yield Failing({0: state()}, [], 'original')
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(SEGMENT, failing, failing, Path(directory))
        self.assertEqual(report['status'], 'blocked')
        self.assertEqual(report['original'], [dict(frame=0, **state())])
        self.assertNotIn('native', report)
        self.assertIn('ordinary boundary stalled', report['reason'])

    def test_mismatched_start_prevents_native_input(self):
        report = self.run_pair({0: state()}, {0: state(7)})
        self.assertEqual(report['status'], 'blocked')
        self.assertFalse(any(row[0] == 'native' for row in self.calls))

    def test_matching_pose_cannot_hide_a_different_saved_task_continuation(self):
        original, native = state(), state()
        original['continuations'] = {'FAVIS': {'cursor': 211, 'tasks': [
            {'instruction': 202, 'counter': 13, 'blocking': True, 'paused': False}]}}
        native['continuations'] = {'FAVIS': {'cursor': 169, 'tasks': [
            {'instruction': 160, 'counter': 3, 'blocking': True, 'paused': False}]}}
        report = self.run_pair({0: original}, {0: native})
        self.assertEqual(report['status'], 'blocked')
        self.assertFalse(any(row[0] == 'native' for row in self.calls))
        fields = {row['field']: row for row in report['start_deltas']}
        self.assertEqual(fields['continuations.FAVIS.cursor']['delta'], -42)
        self.assertEqual(fields['continuations.FAVIS.tasks[0].counter']['delta'], -10)

    def test_missing_script_continuation_rejects_original_before_native_launch(self):
        incomplete = state()
        del incomplete['continuations']
        report = self.run_pair({0: incomplete}, {0: state()})
        self.assertEqual(report['status'], 'blocked')
        self.assertIn('missing observable continuations', report['reason'])
        self.assertNotIn(('open', 'native'), self.calls)

    def test_missing_measurement_never_counts_as_matching(self):
        unknown = state()
        unknown['flags']['talking'] = None
        report = self.run_pair({0: unknown}, {0: unknown})
        self.assertEqual(report['status'], 'blocked')
        self.assertIn('unavailable', {row['status'] for row in report['start_deltas']})

    def test_skipped_original_frame_prevents_native_launch(self):
        report = self.run_pair({0: state()}, {0: state()}, overshoot=True)
        self.assertEqual(report['status'], 'blocked')
        self.assertIn('frame boundary missed', report['reason'])
        self.assertNotIn(('open', 'native'), self.calls)

    def test_late_key_acknowledgement_rejects_the_pair(self):
        calls = []
        @contextmanager
        def late(segment, output):
            class Late(Session):
                def input(self, key, action):
                    return self.frame + 1
            yield Late({0: state()}, calls, 'late')
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(SEGMENT, late, late, Path(directory))
        self.assertEqual(report['status'], 'blocked')
        self.assertIn('input frame missed', report['reason'])

    def test_unexpanded_steering_is_rejected_instead_of_comparing_different_inputs(self):
        with self.assertRaisesRegex(ValueError, 'expanded input'):
            pair.input_tape({'steps': [{'steer': {'position_q8': [1, 2, 3]}}]})


class NativeControl:
    def __init__(self):
        self.update, self.stride, self.failed = 50, 1, 0
        self.events = []
        self.keys = []
        self.scales = []
        self.bios_ticks = []

    def request(self, command):
        if command['type'] == 'state':
            return dict(scripts=dict(ordinary_updates=self.update, failed=self.failed),
                        clock={'continuation': {'step': {'frame_scale': self.scales[-1] if self.scales else 0}}})
        if command['type'] == 'scripts':
            return dict(actors=[], total=0)
        if command['type'] == 'schedule_input':
            self.events.append(command['events'])
            self.update += self.stride
            return {}
        if command['type'] == 'input':
            self.keys.append((self.update, command['key'], command['action']))
            return {}
        if command['type'] == 'replay_frame':
            self.scales.append(command['frame_scale'])
            self.bios_ticks.append(command.get('elapsed_bios_ticks'))
            self.update += self.stride
            return {}
        raise AssertionError(command)


class NativeFrames(unittest.TestCase):
    def test_recorded_clock_rejects_acknowledged_scale_that_was_not_consumed(self):
        control = NativeControl()
        request = control.request
        def wrong_step(command):
            value = request(command)
            if command['type'] == 'state':
                value['clock'] = {'continuation': {'step': {'frame_scale': 7}}}
            return value
        control.request = wrong_step
        session = pair.NativeFrameSession(control, lambda *_: {}, frame_scales=[122, 99])
        with self.assertRaisesRegex(RuntimeError, 'consumed clock scale'):
            session.advance_to(2)
        self.assertEqual(control.scales, [122])

    def test_clock_change_without_an_ordinary_update_rejects_observation(self):
        control = NativeControl()
        request = control.request
        samples = 0
        def moving_clock(command):
            nonlocal samples
            value = request(command)
            if command['type'] == 'state':
                samples += 1
                value['clock'] = {'continuation': {'frames': samples}}
            return value
        control.request = moving_clock
        with self.assertRaisesRegex(RuntimeError, 'clock changed'):
            pair.NativeFrameSession(control, lambda *_: {})

    def test_bios_intervals_are_recorded_not_replaced_by_simulated_time(self):
        control = NativeControl()
        session = pair.NativeFrameSession(control, lambda *_: {}, frame_scales=[122, 99],
                                          bios_ticks=[0, 2])
        session.advance_to(2)
        self.assertEqual(control.bios_ticks, [0, 2])
        self.assertEqual([row['elapsed_scale'] for row in session.clock_observations], [0, 122, 221])
        self.assertEqual(session.clock_observations[-1]['continuation']['step']['frame_scale'], 99)
        for invalid in ([0], [True, 2], [-1, 2], [0, 2**32]):
            with self.assertRaises(ValueError):
                pair.NativeFrameSession(None, None, frame_scales=[122, 99], bios_ticks=invalid)

    def test_counterpart_clock_tape_is_consumed_once_per_frame_without_rounding(self):
        control = NativeControl()
        session = pair.NativeFrameSession(control, lambda state, scripts: {},
                                          frame_scales=[122, 99, 0, 118])
        session.advance_to(1)
        session.advance_to(4)
        self.assertEqual(control.scales, [122, 99, 0, 118])
        self.assertEqual(control.events, [])
        with self.assertRaisesRegex(ValueError, 'exhausted'):
            session.advance_to(5)
        self.assertEqual(control.update, 54)

    def test_clock_tape_is_frozen_and_invalid_scales_reject_before_control_access(self):
        control = NativeControl()
        scales = [99, 122]
        session = pair.NativeFrameSession(control, lambda state, scripts: {}, frame_scales=scales)
        scales[0] = 1
        session.advance_to(2)
        self.assertEqual(control.scales, [99, 122])
        for invalid in ([True], [-1], [1.5]):
            with self.assertRaisesRegex(ValueError, 'integer Q8'):
                pair.NativeFrameSession(None, None, frame_scales=invalid)

    def test_inputs_share_the_boundary_and_frames_only_advance_explicitly(self):
        control = NativeControl()
        session = pair.NativeFrameSession(control, lambda state, scripts: dict(world=20))
        self.assertEqual(session.input('ControlLeft', 'press'), 0)
        session.input('ArrowDown', 'press')
        self.assertEqual(control.update, 50)
        session.advance_to(1)
        self.assertEqual(control.keys, [(50, 'ControlLeft', 'press'), (50, 'ArrowDown', 'press')])
        self.assertEqual(session.input('ControlLeft', 'release'), 1)
        session.advance_to(3)
        self.assertEqual(session.snapshot(), dict(world=20, frame=3))
        self.assertEqual(control.events[-1], [{'after_updates': 0}])

    def test_frame_gap_or_script_failure_rejects_the_native_run(self):
        for stride, failed, message in [(2, 0, 'frame gap'), (1, 1, 'script failures')]:
            control = NativeControl()
            session = pair.NativeFrameSession(control, lambda state, scripts: {})
            control.stride, control.failed = stride, failed
            with self.subTest(message=message), self.assertRaisesRegex(RuntimeError, message):
                session.advance_to(1)

    def test_truncated_actor_batch_is_rejected(self):
        control = NativeControl()
        request = control.request
        def truncated(command):
            return dict(actors=[], total=1) if command['type'] == 'scripts' else request(command)
        control.request = truncated
        with self.assertRaisesRegex(RuntimeError, 'truncated'):
            pair.NativeFrameSession(control, lambda state, scripts: {})


if __name__ == '__main__':
    unittest.main()
