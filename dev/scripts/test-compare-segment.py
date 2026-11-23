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
                flags={'talking': talking}, prompts=[], timing={'elapsed_scale': 0})


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

    def test_mismatched_start_prevents_native_input(self):
        report = self.run_pair({0: state()}, {0: state(7)})
        self.assertEqual(report['status'], 'blocked')
        self.assertFalse(any(row[0] == 'native' for row in self.calls))

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

    def request(self, command):
        if command['type'] == 'state':
            return dict(scripts=dict(ordinary_updates=self.update, failed=self.failed))
        if command['type'] == 'scripts':
            return dict(actors=[], total=0)
        if command['type'] == 'schedule_input':
            self.events.append(command['events'])
            self.update += self.stride
            return {}
        if command['type'] == 'input':
            self.keys.append((self.update, command['key'], command['action']))
            return {}
        raise AssertionError(command)


class NativeFrames(unittest.TestCase):
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
