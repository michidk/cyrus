#!/usr/bin/env python3
"""Exercise paired execution, wrong-version detection and invalid timing admission."""
from contextlib import contextmanager
import copy
import importlib.util
import hashlib
import json
import subprocess
import sys
from pathlib import Path
import tempfile
import unittest
from unittest import mock

spec = importlib.util.spec_from_file_location('pair', Path(__file__).with_name('compare-segment.py'))
pair = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pair)

SEGMENT = {'steps': [{'input': [{'after_updates': 0, 'key': 'ControlLeft', 'action': 'press'},
                              {'after_updates': 2, 'key': 'ControlLeft', 'action': 'release'}]},
                     {'checkpoint': {}}]}


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


class RestorationPhaseTests(unittest.TestCase):
    def witness(self):
        return dict(initial_state=1, events=[
            dict(type='write', draw=1, previous=1, state=1103527590,
                 phase='reload', context=dict(world=20)),
            dict(type='boundary', name='restored-publication', state=1103527590,
                 draw=1, context=dict(world=20)),
            dict(type='write', draw=2, previous=1103527590, state=2524885223,
                 phase='ordinary', context=dict(world=20)),
            dict(type='boundary', name='first-completed-body', state=2524885223,
                 draw=2, context=dict(world=20))])

    def run_phases(self, native_entry=1103527590, native_body=2524885223, witness=None):
        calls = []
        @contextmanager
        def factory(name, rng):
            class Measured(Session):
                def advance_to(self, frame):
                    calls.append(('advance', name, frame))
                    super().advance_to(frame)
            yield Measured({0: dict(state(), random_state=rng),
                            1: dict(state(), random_state=(native_body if name == 'native' else 2524885223)),
                            3: dict(state(), random_state=2524885223)}, calls, name)
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(SEGMENT, lambda *_: factory('original', 1103527590),
                lambda *_: factory('native', native_entry), Path(directory),
                restoration_history=self.witness() if witness is None else witness)
        return report, calls

    def test_restored_and_first_acknowledged_body_are_separate_gates(self):
        report, calls = self.run_phases()
        self.assertEqual(report['status'], 'match', report)
        phases = report['restoration_phases']
        for side in ('original', 'native'):
            self.assertEqual(phases[side]['restored-publication']['measured']['frame'], 0)
            self.assertEqual(phases[side]['first-completed-body']['measured']['frame'], 1)
        first_input = next(i for i, row in enumerate(calls) if row[0] in ('original', 'native'))
        self.assertTrue(all(calls.index(('advance', side, 1)) < first_input
                            for side in ('original', 'native')))

    def test_premature_body_endpoint_rejects_before_any_advance_or_input(self):
        report, calls = self.run_phases(native_entry=2524885223)
        self.assertEqual(report['status'], 'blocked', report)
        self.assertIn('native restored-publication', report['reason'])
        self.assertIn('first-completed-body', report['reason'])
        self.assertEqual(calls, [])
        self.assertEqual(report['restoration_phases']['native']['restored-publication']
                         ['measured']['random_state'], 2524885223)

    def test_wrong_first_body_rejects_without_combat_input(self):
        report, calls = self.run_phases(native_body=1103527590)
        self.assertEqual(report['status'], 'blocked', report)
        self.assertIn('native first-completed-body', report['reason'])
        self.assertFalse(any(row[0] in ('original', 'native') for row in calls))

    def test_missing_final_write_or_boundary_cannot_be_an_expected_endpoint(self):
        for index in (-2, -1):
            witness = self.witness(); del witness['events'][index]
            report, calls = self.run_phases(witness=witness)
            self.assertEqual(report['status'], 'blocked', report)
            self.assertEqual(calls, [])
            self.assertIn('RNG', report['reason'])

    def test_invalid_count_order_and_missing_initial_state_reject_before_open(self):
        for change in ('count', 'order', 'initial'):
            witness = self.witness()
            if change == 'count': witness['events'][0]['draw'] = 2
            if change == 'order': witness['events'].reverse()
            if change == 'initial': del witness['initial_state']
            report, calls = self.run_phases(witness=witness)
            self.assertEqual(report['status'], 'blocked', report)
            self.assertEqual(calls, [])


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
        lifecycle = [row for row in self.calls if row[0] in ('open', 'close')]
        self.assertEqual(lifecycle, [('open', 'original'), ('close', 'original'),
                                     ('open', 'native'), ('close', 'native')] * 2)
        last_original_close = len(self.calls) - 1 - self.calls[::-1].index(('close', 'original'))
        native_input = next(i for i, row in enumerate(self.calls) if row[0] == 'native')
        self.assertLess(last_original_close, native_input)
        self.assertEqual([row[1:] for row in self.calls if row[0] == 'original'],
                         [(0, 'ControlLeft', 'press'), (2, 'ControlLeft', 'release')])
        self.assertEqual([row[1:] for row in self.calls if row[0] == 'original'],
                         [row[1:] for row in self.calls if row[0] == 'native'])

    def test_entry_mismatch_retains_both_states_without_advancing_either_backend(self):
        calls = []
        @contextmanager
        def factory(name, value):
            class Stopped(Session):
                def advance_to(self, frame):
                    calls.append(('advance', name, frame))
                    super().advance_to(frame)
            yield Stopped({0: value}, calls, name)
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(SEGMENT, lambda *_: factory('original', state()),
                                   lambda *_: factory('native', state(7)), Path(directory))
        self.assertEqual(report['status'], 'blocked')
        self.assertEqual(calls, [])
        self.assertEqual(report['entry']['original'], dict(frame=0, **state()))
        self.assertEqual(report['entry']['native'], dict(frame=0, **state(7)))
        fields = {row['field']: row for row in report['start_deltas']}
        self.assertEqual(fields['player.position_q8[0]']['delta'], 7)

    def test_temporary_source_difference_blocks_even_with_identical_registry_actors(self):
        source = dict(kind='separate_temporary', lifetime='process_global',
                      state=dict(attributes={'Health': 0}, position_q8=[1, 2, 3],
                                 orientation=[1, 0, 0],
                                 script_context=dict(name='ambient', main=0, local_offset=4)))
        for field, value in [('attributes', {'Health': 1}), ('position_q8', [1, 2, 4]),
                             ('orientation', [0, 1, 0]),
                             ('script_context', dict(name='ambient', main=1, local_offset=4))]:
            with self.subTest(field=field):
                original = state(); original['native_source'] = copy.deepcopy(source)
                native = copy.deepcopy(original); native['native_source']['state'][field] = value
                report = self.run_pair({0: original}, {0: native})
                self.assertEqual(report['status'], 'blocked', report)
                self.assertTrue(any(row['field'].startswith('native_source.state.' + field)
                                    for row in report['admission_rejections'][0]['fields']))
                self.assertFalse(any(row[0] in ('original', 'native') for row in self.calls))

    def test_missing_or_unobserved_source_cannot_pass_a_projected_source(self):
        original = state()
        original['native_source'] = dict(kind='separate_temporary', state={'attributes': {'Health': 0}})
        for native in (state(), dict(copy.deepcopy(original), native_source=None),
                       dict(copy.deepcopy(original), native_source=dict(kind='registry_actor', state={})),
                       dict(copy.deepcopy(original), native_source=dict(kind='separate_temporary',
                                                                        state={'attributes': None}))):
            with self.subTest(native=native):
                report = self.run_pair({0: original}, {0: native})
                self.assertEqual(report['status'], 'blocked', report)
                self.assertTrue(any(row['field'].startswith('native_source')
                                    for row in report['admission_rejections'][0]['fields']))

    def test_equal_observed_source_passes_but_equal_unknown_source_does_not(self):
        observed = state()
        observed['native_source'] = dict(kind='separate_temporary', lifetime='process_global',
                                         state={'attributes': {'Health': 0}})
        self.assertEqual(self.run_pair({0: observed}, {0: observed})['status'], 'match')
        unknown = copy.deepcopy(observed); unknown['native_source']['state']['orientation'] = None
        report = self.run_pair({0: unknown}, {0: unknown})
        self.assertEqual(report['status'], 'blocked')
        self.assertIn('native_source.state.orientation', report['reason'])

    def test_reopened_temporary_source_drift_rejects_before_inputs(self):
        calls, opens = [], dict(original=0, native=0)
        @contextmanager
        def factory(name):
            opens[name] += 1
            observed = state()
            health = int(name == 'native' and opens[name] == 2)
            observed['native_source'] = dict(kind='separate_temporary',
                                             state={'attributes': {'Health': health}})
            yield Session({0: observed}, calls, name)
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(SEGMENT, lambda *_: factory('original'),
                                   lambda *_: factory('native'), Path(directory))
        self.assertEqual(report['status'], 'blocked')
        self.assertIn('native_source.state.attributes.Health', report['reason'])
        self.assertFalse(any(row[0] == 'native' for row in calls))

    def test_reopened_entry_drift_blocks_before_that_sides_inputs(self):
        for changed in ('original', 'native'):
            with self.subTest(changed=changed):
                calls, opens = [], dict(original=0, native=0)
                @contextmanager
                def factory(name):
                    opens[name] += 1
                    value = state(7 if name == changed and opens[name] == 2 else 0)
                    yield Session({0: value}, calls, name)
                with tempfile.TemporaryDirectory() as directory:
                    report = pair.run_pair(SEGMENT, lambda *_: factory('original'),
                                           lambda *_: factory('native'), Path(directory))
                self.assertEqual(report['status'], 'blocked', report)
                self.assertIn(changed + ' replay entry differs', report['reason'])
                self.assertFalse(any(row[0] == changed for row in calls))
                self.assertEqual(report['replay_entry'][changed]['player']['position_q8'][0], 7)

    def test_malformed_entry_is_retained_with_side_and_phase_context(self):
        for side in ('original', 'native'):
            with self.subTest(side=side):
                @contextmanager
                def factory(name):
                    session = Session({0: state()}, [], name)
                    if name == side:
                        session.snapshot = lambda: {'frame': 0, 'world': 20}
                    yield session
                with tempfile.TemporaryDirectory() as directory:
                    report = pair.run_pair(SEGMENT, lambda *_: factory('original'),
                                           lambda *_: factory('native'), Path(directory))
                self.assertEqual(report['status'], 'blocked')
                self.assertIn(side + ' entry: frame 0: missing observable player', report['reason'])
                self.assertEqual(report['entry'][side], {'frame': 0, 'world': 20})
                self.assertNotIn('original', report)

    def test_rejected_native_preflight_retains_execution_evidence_separately(self):
        evidence = {'clocks': [{'frame': 0}], 'inputs': []}
        @contextmanager
        def original(*_):
            yield Session({0: state()}, [], 'original')
        @contextmanager
        def native(*_):
            session = Session({0: state()}, [], 'native')
            session.execution_evidence = lambda: evidence
            session.snapshot = lambda: {'frame': 0}
            yield session
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(SEGMENT, original, native, Path(directory))
        self.assertEqual(report['status'], 'blocked')
        self.assertEqual(report['entry_native_execution'], evidence)
        self.assertNotIn('native_execution', report)
        evidence['clocks'].clear()
        self.assertEqual(report['entry_native_execution']['clocks'], [{'frame': 0}])

    def test_projected_shared_state_cannot_be_silently_dropped_from_admission(self):
        for field in ('random_state', 'globals', 'inventory'):
            for missing in (False, True):
                with self.subTest(field=field, missing=missing):
                    original, native = state(), state()
                    original[field] = 111 if field == 'random_state' else {'shared': 111}
                    if not missing:
                        native[field] = 222 if field == 'random_state' else {'shared': 222}
                    report = self.run_pair({0: original}, {0: native})
                    self.assertEqual(report['status'], 'blocked', report)
                    self.assertFalse(any(row[0] in ('original', 'native') for row in self.calls))
                    rejection = report['admission_rejections'][0]
                    self.assertEqual(rejection['phase'], 'entry')
                    self.assertTrue(rejection['fields'][0]['field'].startswith(field))
                    self.assertIn(field, report['reason'])
                    self.assertEqual(report['input_attempts'], dict(original=[], native=[]))

    def test_shared_state_is_compared_after_admission_and_on_reopen(self):
        for field in ('globals', 'inventory', 'random_state'):
            for phase in ('reopen', 'checkpoint'):
                with self.subTest(field=field, phase=phase):
                    calls, opens = [], dict(original=0, native=0)
                    initial = dict(state(), **{field: 111 if field == 'random_state' else {'shared': 111}})
                    changed = dict(state(), **{field: 222 if field == 'random_state' else {'shared': 222}})
                    @contextmanager
                    def factory(name):
                        opens[name] += 1
                        first = changed if phase == 'reopen' and name == 'native' and opens[name] == 2 else initial
                        last = changed if phase == 'checkpoint' and name == 'native' else initial
                        yield Session({0: first, 3: last}, calls, name)
                    with tempfile.TemporaryDirectory() as directory:
                        report = pair.run_pair(SEGMENT, lambda *_: factory('original'),
                                               lambda *_: factory('native'), Path(directory))
                    self.assertEqual(report['status'], 'blocked' if phase == 'reopen' else 'different', report)
                    fields = (report['admission_rejections'][-1]['fields'] if phase == 'reopen'
                              else report['checkpoints'][-1]['fields'])
                    self.assertTrue(any(row['field'].startswith(field) and row['status'] == 'different' for row in fields))

    def test_acquisition_histories_are_retained_without_requiring_equal_histories(self):
        history = {'original': {'preload': {'world': 1, 'updates': 9}},
                   'native': {'preload': {'world': 1, 'updates': 2}}}
        @contextmanager
        def factory(side):
            session = Session({0: state()}, [], side)
            session.acquisition_evidence = lambda: history[side]
            yield session
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(SEGMENT, lambda *_: factory('original'),
                                   lambda *_: factory('native'), Path(directory))
        self.assertEqual(report['status'], 'match', report)
        for phase in ('entry', 'replay_entry'):
            self.assertEqual(report['acquisition'][phase], history)
        history['original']['preload']['updates'] = 100
        self.assertEqual(report['acquisition']['entry']['original']['preload']['updates'], 9)

    def test_acquisition_evidence_survives_rejected_and_malformed_entry(self):
        for malformed in (False, True):
            @contextmanager
            def factory(side):
                value = dict(state(), random_state=111 if side == 'original' else 222)
                session = Session({0: value}, [], side)
                session.acquisition_evidence = lambda: {'preload_updates': 9 if side == 'original' else 2}
                if malformed and side == 'native':
                    session.snapshot = lambda: {'frame': 0}
                yield session
            with self.subTest(malformed=malformed), tempfile.TemporaryDirectory() as directory:
                report = pair.run_pair(SEGMENT, lambda *_: factory('original'),
                                       lambda *_: factory('native'), Path(directory))
            self.assertEqual(report['status'], 'blocked', report)
            self.assertEqual(report['acquisition']['entry'], {
                'original': {'preload_updates': 9}, 'native': {'preload_updates': 2}})
            if not malformed:
                field = report['admission_rejections'][0]['fields'][0]
                self.assertEqual((field['field'], field['original'], field['native']), ('random_state', 111, 222))

    def test_invalid_acquisition_evidence_blocks_with_side_and_phase(self):
        for invalid in (None, [], 'failed'):
            @contextmanager
            def factory(*_):
                session = Session({0: state()}, [], 'original')
                def acquisition():
                    if invalid == 'failed':
                        raise RuntimeError('acquisition transport closed')
                    return invalid
                session.acquisition_evidence = acquisition
                yield session
            with self.subTest(invalid=invalid), tempfile.TemporaryDirectory() as directory:
                report = pair.run_pair(SEGMENT, factory, factory, Path(directory))
            self.assertEqual(report['status'], 'blocked', report)
            self.assertIn('original entry', report['reason'])
            self.assertIn('acquisition', report['reason'])
            if invalid == 'failed':
                self.assertEqual(report['acquisition']['entry']['original']['error'], 'acquisition transport closed')
            else:
                self.assertEqual(report['acquisition']['entry']['original'], invalid)

    def test_report_distinguishes_requested_tape_from_accepted_inputs(self):
        report = self.run_pair({0: state()}, {0: state()})
        self.assertEqual(report['status'], 'match')
        for side in ('original', 'native'):
            self.assertEqual(report['input_attempts'][side], [
                dict(event, acknowledged_frame=event['frame'], status='accepted')
                for event in report['events']])
        blocked = self.run_pair({0: state()}, {0: state(7)})
        self.assertEqual(blocked['input_attempts'], dict(original=[], native=[]))
        self.assertEqual(len(blocked['events']), 2)

    def test_rejected_or_failed_input_retains_partial_receipts_for_each_side(self):
        for side in ('original', 'native'):
            for reply in (3, None, True, 'failure'):
                with self.subTest(side=side, reply=reply):
                    @contextmanager
                    def factory(name):
                        class Rejected(Session):
                            def input(self, key, action):
                                if name == side and action == 'release':
                                    if reply == 'failure':
                                        raise TimeoutError('key transport closed')
                                    return reply
                                return super().input(key, action)
                        yield Rejected({0: state()}, [], name)
                    with tempfile.TemporaryDirectory() as directory:
                        report = pair.run_pair(SEGMENT, lambda *_: factory('original'),
                                               lambda *_: factory('native'), Path(directory))
                    self.assertEqual(report['status'], 'blocked')
                    attempts = report['input_attempts'][side]
                    self.assertEqual(attempts[0]['status'], 'accepted')
                    self.assertEqual(attempts[0]['acknowledged_frame'], 0)
                    self.assertEqual(attempts[1]['frame'], 2)
                    self.assertEqual(attempts[1]['action'], 'release')
                    if reply == 'failure':
                        self.assertEqual(attempts[1]['status'], 'failed')
                        self.assertEqual(attempts[1]['reason'], 'key transport closed')
                        self.assertNotIn('acknowledged_frame', attempts[1])
                    else:
                        self.assertEqual(attempts[1]['status'], 'rejected')
                        self.assertIs(attempts[1]['acknowledged_frame'], reply)
                    if side == 'original':
                        self.assertEqual(report['input_attempts']['native'], [])
                    json.dumps(report)

    def test_unsupported_checkpoint_expectations_cannot_be_silently_accepted(self):
        for expected in ({'expression': 'health < 10'}, {'fight': {'target_health': 0}},
                         {'position_q8': {'value': [0, 0, 0], 'tolerance': 1, 'unknown': 0}}):
            with self.subTest(expected=expected):
                segment = copy.deepcopy(SEGMENT)
                segment['steps'][-1]['checkpoint'] = expected
                calls = []
                @contextmanager
                def factory(*_):
                    calls.append('opened')
                    yield Session({0: state()}, [], 'backend')
                with tempfile.TemporaryDirectory() as directory:
                    report = pair.run_pair(segment, factory, factory, Path(directory))
                self.assertEqual(report['status'], 'blocked', report)
                self.assertIn('step 1:', report['reason'])
                self.assertIn('unsupported', report['reason'])
                self.assertIn(next(iter(expected)), report['reason'])
                self.assertEqual(calls, [])

    def test_malformed_checkpoint_is_rejected_before_backend_launch(self):
        for expected in (None, [], '', False):
            with self.subTest(expected=expected):
                segment = copy.deepcopy(SEGMENT)
                segment['steps'][-1]['checkpoint'] = expected
                calls = []
                @contextmanager
                def factory(*_):
                    calls.append('opened')
                    yield Session({0: state()}, [], 'backend')
                with tempfile.TemporaryDirectory() as directory:
                    report = pair.run_pair(segment, factory, factory, Path(directory))
                self.assertEqual(report['status'], 'blocked', report)
                self.assertIn('step 1: paired checkpoint must be an object', report['reason'])
                self.assertEqual(calls, [])

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

    def test_skipped_original_replay_frame_prevents_native_replay(self):
        report = self.run_pair({0: state()}, {0: state()}, overshoot=True)
        self.assertEqual(report['status'], 'blocked')
        self.assertIn('frame boundary missed', report['reason'])
        self.assertEqual(self.calls.count(('open', 'native')), 1)
        self.assertFalse(any(row[0] == 'native' for row in self.calls))

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

    def test_exact_duration_releases_at_final_boundary_without_an_extra_body(self):
        segment = {'steps': [dict(updates=2, input=[
            dict(after_updates=0, key='ControlLeft', action='press'),
            dict(after_updates=2, key='ControlLeft', action='release')]),
            {'checkpoint': dict(opponent='FAVIS', target_health=90)}]}
        calls = []
        @contextmanager
        def factory(segment, output):
            initial = state()
            final = copy.deepcopy(initial)
            final['actors']['FAVIS']['health'] = 90
            yield Session({0: initial, 2: final}, calls, 'recorded')
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(segment, factory, factory, Path(directory))
        self.assertEqual(report['status'], 'match', report)
        self.assertEqual([x['frame'] for x in report['original']], [0, 2])
        self.assertEqual([x['frame'] for x in report['input_attempts']['original']], [0, 2])
        self.assertEqual(report['input_attempts']['original'][-1]['action'], 'release')
        self.assertTrue(all(row['status'] == 'match'
                            for outcome in report['outcomes'] for row in outcome['fields']))

    def test_explicit_duration_rejects_unobserved_offsets_and_invalid_counts(self):
        for updates in (-1, True, 1.5, 1):
            with self.subTest(updates=updates), self.assertRaisesRegex(ValueError, 'updates'):
                pair.input_tape({'steps': [dict(updates=updates, input=[
                    dict(after_updates=2, key='ControlLeft', action='release')])]})
        events, checkpoints, _ = pair.input_tape({'steps': [dict(updates=0, input=[
            dict(after_updates=0, key='ControlLeft', action='release')])]})
        self.assertEqual(checkpoints, [0])
        self.assertEqual(events[0]['frame'], 0)

    def test_unexpanded_steering_is_rejected_instead_of_comparing_different_inputs(self):
        with self.assertRaisesRegex(ValueError, 'expanded input'):
            pair.input_tape({'steps': [{'steer': {'position_q8': [1, 2, 3]}}]})


class OutcomeTests(unittest.TestCase):
    def compare(self, expected, original=None, native=None, initial=None):
        initial = state() if initial is None else initial
        original = state() if original is None else original
        native = copy.deepcopy(original) if native is None else native
        segment = copy.deepcopy(SEGMENT)
        segment['steps'][-1]['checkpoint'] = expected
        segment['steps'].append({'input': [{'after_updates': 0, 'key': 'ArrowDown', 'action': 'press'}]})
        self.calls = []
        @contextmanager
        def factory(name, final):
            self.calls.append(('open', name))
            yield Session({0: initial, 3: final}, self.calls, name)
        with tempfile.TemporaryDirectory() as directory:
            return pair.run_pair(segment, lambda *_: factory('original', original),
                                 lambda *_: factory('native', native), Path(directory))

    def test_route_checkpoint_evaluates_world_flags_inventory_globals_health_and_full_pose(self):
        final = state(x=5)
        final.update(flags={'talking': {'value': 1, 'latched': False}},
                     globals={'lockout': 0}, inventory={'1': 1, '79': 1})
        expected = dict(world=20, flags={'talking': {'value': 1}}, globals={'lockout': 0},
                        inventory={'79': 1}, health=100,
                        position_q8={'value': [5, -16384, 587520], 'tolerance': 1})
        report = self.compare(expected, final)
        self.assertEqual(report['status'], 'match', report)
        self.assertEqual({row['side'] for row in report['outcomes']}, {'original', 'native'})
        self.assertTrue(all(field['status'] == 'match' for row in report['outcomes'] for field in row['fields']))

    def test_two_equally_wrong_flags_fail_outcomes_before_later_input(self):
        final = state()
        final['flags'] = {'talking': {'value': 0}}
        report = self.compare({'flags': {'talking': {'value': 1}}}, final)
        self.assertEqual(report['status'], 'different')
        self.assertIn('original frame 3', report['reason'])
        field = report['outcomes'][-1]['fields'][0]
        self.assertEqual((field['field'], field['expected'], field['actual']), ('flags.talking.value', 1, 0))
        self.assertNotIn(('original', 3, 'ArrowDown', 'press'), self.calls)
        self.assertFalse(any(row[0] == 'native' for row in self.calls))

    def test_native_outcome_is_checked_independently_and_stops_before_later_input(self):
        report = self.compare({'health': 100}, state(), dict(state(), player=dict(state()['player'], health=90)))
        self.assertEqual(report['status'], 'different')
        self.assertIn('native frame 3', report['reason'])
        self.assertEqual([row['side'] for row in report['outcomes']], ['original', 'native'])
        self.assertNotIn(('native', 3, 'ArrowDown', 'press'), self.calls)

    def test_retreat_pose_tolerance_includes_height_and_boundary_is_inclusive(self):
        expected = {'position_q8': {'value': [5, -16384, 587520], 'tolerance': 5}}
        final = state(5)
        final['player']['position_q8'][1] += 5
        self.assertEqual(self.compare(expected, final)['status'], 'match')
        final['player']['position_q8'][1] += 1
        report = self.compare(expected, final)
        self.assertEqual(report['status'], 'different')
        self.assertEqual(report['outcomes'][-1]['fields'][0]['distance_q8'], 6)

    def test_retreat_asserts_exact_actor_health_and_three_dimensional_separation(self):
        final = state()
        final['player']['position_q8'] = [1, 2, 3]
        final['actors']['FAVIS']['position_q8'] = [5, 8, 12]
        expected = dict(health=100, actor_health={'FAVIS': 100},
                        separation_l1_q8={'opponent': 'FAVIS', 'value': 19},
                        position_q8={'value': [1, 2, 3], 'tolerance': 0})
        self.assertEqual(self.compare(expected, final)['status'], 'match')
        for section, key, value in (('player', 'health', 90),
                                    ('actors', 'FAVIS', {'health': 99, 'position_q8': [5, 8, 12]}),
                                    ('player', 'position_q8', [1, 3, 3])):
            changed = copy.deepcopy(final)
            changed[section][key] = value
            report = self.compare(expected, changed)
            self.assertEqual(report['status'], 'different', report)
            self.assertNotIn(('original', 3, 'ArrowDown', 'press'), self.calls)

    def test_retreat_missing_or_invalid_observations_do_not_pass(self):
        expected = {'separation_l1_q8': {'opponent': 'FAVIS', 'value': 19}}
        for bad in (None, [1, 2], [True, 2, 3]):
            final = state()
            final['actors']['FAVIS']['position_q8'] = bad
            self.assertEqual(self.compare(expected, final)['status'], 'blocked')
        self.assertEqual(self.compare({'actor_health': {'MISSING': 100}})['status'], 'blocked')
        for invalid in ({'actor_health': {}}, {'actor_health': {'FAVIS': True}},
                        {'separation_l1_q8': {'opponent': 'FAVIS', 'value': -1}},
                        {'separation_l1_q8': {'opponent': '', 'value': 0}},
                        {'separation_l1_q8': {'opponent': 'FAVIS', 'value': True}},
                        {'separation_l1_q8': {'opponent': 'FAVIS', 'value': 1, 'tolerance': 2}}):
            report = self.compare(invalid)
            self.assertEqual(report['status'], 'blocked')
            self.assertEqual(self.calls, [])

    def test_hit_target_requires_observed_health_loss_and_player_survival(self):
        expected = {'opponent': 'FAVIS', 'target_health': 90}
        hit = state()
        hit['actors']['FAVIS']['health'] = 90
        self.assertEqual(self.compare(expected, hit)['status'], 'match')
        for initial, final in ((state(), state()), (hit, hit),
                               (state(), dict(hit, player=dict(hit['player'], health=0)))):
            with self.subTest(initial=initial['actors'], final=final['player']['health']):
                report = self.compare(expected, final, initial=initial)
                self.assertEqual(report['status'], 'different')
                self.assertTrue(any(row['status'] == 'different' for row in report['outcomes'][-1]['fields']))

    def test_missing_measured_assertion_fields_are_unavailable_and_do_not_pass(self):
        for expected in ({'inventory': {'79': 1}}, {'globals': {'lockout': 0}},
                         {'opponent': 'MISSING', 'target_health': 0}):
            with self.subTest(expected=expected):
                report = self.compare(expected)
                self.assertEqual(report['status'], 'blocked')
                self.assertIn('unavailable', {row['status'] for row in report['outcomes'][-1]['fields']})

    def test_malformed_supported_assertions_reject_before_any_backend_opens(self):
        for expected in ({'world': True}, {'health': {'opponent': 0}}, {'flags': {}},
                         {'flags': {'talking': {'value': True}}}, {'globals': {'lockout': None}},
                         {'inventory': {'79': False}}, {'position_q8': {'value': [0, 0]}},
                         {'position_q8': {'value': [0, 0, 0], 'tolerance': float('nan')}},
                         {'position_q8': {'value': [0, 0, 0], 'tolerance': -1}},
                         {'opponent': 'FAVIS'}, {'target_health': 90},
                         {'opponent': '', 'target_health': 90}):
            with self.subTest(expected=expected):
                report = self.compare(expected)
                self.assertEqual(report['status'], 'blocked')
                self.assertIn('step 1:', report['reason'])
                self.assertEqual(self.calls, [])

    def test_frame_zero_and_multiple_assertions_at_one_boundary_are_all_checked(self):
        segment = copy.deepcopy(SEGMENT)
        segment['steps'].insert(0, {'checkpoint': {'world': 20}})
        segment['steps'] += [{'checkpoint': {'health': 100}}, {'checkpoint': {'world': 21}}]
        calls = []
        @contextmanager
        def backend(*_):
            calls.append('open')
            yield Session({0: state()}, calls, 'original')
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(segment, backend, backend, Path(directory))
        self.assertEqual(report['status'], 'different', report)
        self.assertEqual([(row['frame'], row['step']) for row in report['outcomes']],
                         [(0, 0), (3, 3), (3, 4)])
        self.assertEqual(report['outcomes'][-1]['fields'][0]['actual'], 20)
        self.assertEqual(calls.count('open'), 3)  # Two preflights, then original replay.

    def test_invalid_measurement_types_are_unavailable_instead_of_coerced(self):
        for expected, section, key, bad in (
                ({'health': 1}, 'player', 'health', True),
                ({'position_q8': {'value': [0, 0, 0], 'tolerance': 5}},
                 'player', 'position_q8', [False, 0, 0]),
                ({'flags': {'talking': {'value': 1}}}, 'flags', 'talking', 1),
                ({'flags': {'talking': {'latched': False}}}, 'flags', 'talking', {'latched': 0})):
            with self.subTest(expected=expected):
                final = state()
                final[section][key] = bad
                report = self.compare(expected, final)
                self.assertEqual(report['status'], 'blocked', report)
                self.assertEqual(report['outcomes'][-1]['fields'][0]['status'], 'unavailable')

    def test_missing_initial_opponent_health_cannot_establish_damage(self):
        initial, final = state(), state()
        del initial['actors']['FAVIS']['health']
        final['actors']['FAVIS']['health'] = 90
        report = self.compare({'opponent': 'FAVIS', 'target_health': 90}, final, initial=initial)
        self.assertEqual(report['status'], 'blocked', report)
        field = next(row for row in report['outcomes'][-1]['fields']
                     if row['field'].endswith('health_decreased'))
        self.assertEqual(field['status'], 'unavailable')
        self.assertIsNone(field['initial_health'])


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
            return dict(key=command['key'], action=command['action'])
        if command['type'] == 'replay_frame':
            self.scales.append(command['frame_scale'])
            self.bios_ticks.append(command.get('elapsed_bios_ticks'))
            self.update += self.stride
            return {}
        raise AssertionError(command)


class NativeClockTimelines(unittest.TestCase):
    def timeline(self):
        return dict(stored_sample=101, world=30,
                    decisions=[dict(index=43, name='GUARDNS3', occurrence=1)],
                    before_decisions=[145], after_decisions=[147])

    def session(self, *, timeline=None, generation=7, final_sample=147, change_ack=None):
        control = NativeControl(); original = control.request; sample = [100]; commands = []
        def request(command):
            if command['type'] == 'replay_frame':
                commands.append(copy.deepcopy(command)); sample[0] = final_sample
            result = original(command)
            if command['type'] == 'state': result['clock']['bios_sample'] = sample[0]
            if command['type'] == 'scripts': result['generation'] = generation
            if command['type'] == 'replay_frame':
                result = dict(boundaries=[dict(generation=generation, update=control.update,
                                              after_updates=0, key=None, action=None)])
                if change_ack is not None: result = change_ack(result)
            return result
        control.request = request
        session = pair.NativeFrameSession(control, lambda *_: {}, frame_scales=[202], bios_ticks=[1],
                    cached_bios_samples=[100, 147], clock_timelines=[timeline or self.timeline()])
        return session, control, commands

    def test_supported_timeline_keeps_effective_input_and_cas_separate(self):
        session, control, commands = self.session()
        session.advance_to(1)
        expected = dict(self.timeline(), generation=7, expected_update=50, expected_bios_sample=100)
        self.assertEqual(commands[0]['clock_timeline'], expected)
        self.assertEqual(commands[0]['elapsed_bios_ticks'], 1)
        self.assertEqual(session.execution_evidence()['timeline_observations'][0]['acknowledgement']['boundaries'][0]['generation'], 7)
        self.assertEqual(control.bios_ticks, [1])

    def test_missing_stale_or_wrong_boundary_reply_rejects_even_when_final_clock_matches(self):
        changes = [lambda ack: {}, lambda ack: None,
                   lambda ack: dict(boundaries=[]),
                   lambda ack: dict(boundaries=ack['boundaries'] * 2)]
        for field, value in [('generation', 8), ('generation', True), ('update', 50),
                             ('after_updates', 1), ('key', 'ArrowDown'), ('action', 'press')]:
            changes.append(lambda ack, field=field, value=value:
                           dict(boundaries=[dict(ack['boundaries'][0], **{field: value})]))
        for change in changes:
            session, _, _ = self.session(change_ack=change)
            with self.subTest(change=change), self.assertRaisesRegex(RuntimeError, 'acknowledgement'):
                session.advance_to(1)
            evidence = session.execution_evidence()['timeline_observations'][0]
            self.assertEqual(evidence['observed']['clock']['bios_sample'], 147)
            self.assertEqual(evidence['observed']['ordinary_update'], 51)
            self.assertIn('acknowledgement', evidence)

    def test_missing_input_receipt_fields_do_not_prove_released_inputs(self):
        for field in ('key', 'action'):
            def omit(ack, field=field):
                del ack['boundaries'][0][field]
                return ack
            session, _, _ = self.session(change_ack=omit)
            with self.subTest(field=field), self.assertRaisesRegex(RuntimeError, 'acknowledgement'):
                session.advance_to(1)
            evidence = session.execution_evidence()['timeline_observations'][0]
            self.assertNotIn(field, evidence['acknowledgement']['boundaries'][0])
            self.assertEqual(evidence['observed']['clock']['bios_sample'], 147)

    def test_generation_change_after_reply_retains_actual_rejected_boundary(self):
        session, control, _ = self.session()
        request = control.request
        def changed(command):
            result = request(command)
            if command['type'] == 'scripts' and control.update == 51:
                result['generation'] = 8
            return result
        control.request = changed
        with self.assertRaisesRegex(RuntimeError, 'generation changed during frame'):
            session.advance_to(1)
        observed = session.execution_evidence()['timeline_observations'][0]['observed']
        self.assertEqual(observed['generation'], 8)
        self.assertEqual(observed['clock']['bios_sample'], 147)

    def test_malformed_missing_or_misaligned_fixture_rejects_before_execution(self):
        bad = [dict(self.timeline(), stored_sample=True), dict(self.timeline(), decisions=[]),
               dict(self.timeline(), before_decisions=[-1]), dict(self.timeline(), after_decisions=[149]),
               dict(self.timeline(), decisions=[dict(index=43, name='GUARDNS3', occurrence=2)])]
        for timeline in bad:
            with self.subTest(timeline=timeline), self.assertRaises(ValueError): self.session(timeline=timeline)
        with self.assertRaisesRegex(ValueError, 'actual native script generation'):
            self.session(generation=None)
        with self.assertRaisesRegex(ValueError, 'effective interval'):
            pair.NativeFrameSession(NativeControl(), lambda *_: {}, frame_scales=[202],
                 cached_bios_samples=[100, 147], clock_timelines=[self.timeline()])

    def test_stale_generation_and_wrong_actual_endpoint_retain_rejected_evidence(self):
        session, control, commands = self.session(final_sample=149)
        with self.assertRaisesRegex(RuntimeError, 'cached BIOS phase'): session.advance_to(1)
        self.assertEqual(len(session.execution_evidence()['timeline_observations']), 1)
        session, control, commands = self.session()
        session.current[1]['generation'] = 8
        with self.assertRaisesRegex(RuntimeError, 'generation changed'): session.advance_to(1)
        self.assertEqual(commands, [])


class BiosOriginFixtures(unittest.TestCase):
    def test_continuous_origin_wrap_and_zero_intervals(self):
        self.assertEqual(pair.validate_bios_origin_tape([100, 100, 102], [0, 2]), 100)
        self.assertEqual(pair.validate_bios_origin_tape([2**32 - 1, 0, 1], [1, 1]), 2**32 - 1)
        self.assertEqual(pair.validate_bios_origin_tape([100], []), 100)

    def test_absolute_gap_cannot_be_hidden_by_equal_effective_intervals(self):
        samples, intervals = [100, 100, 145], [0, 2]
        with self.assertRaisesRegex(ValueError, 'body 2.*sample 145.*interval 102'):
            pair.validate_bios_origin_tape(samples, intervals)
        self.assertEqual(samples, [100, 100, 145])
        self.assertEqual(intervals, [0, 2])
        self.assertNotEqual((145 + 3) % 29, (102 + 3) % 29)

    def test_missing_and_invalid_origin_fixture_rejects(self):
        for samples, intervals in [([], []), ([100], [1]), ([True], []),
                                   ([-1], []), ([2**32], []), ([100, 101], [True]),
                                   (None, []), ([100], None)]:
            with self.subTest(samples=samples, intervals=intervals), self.assertRaises(ValueError):
                pair.validate_bios_origin_tape(samples, intervals)


class NativeFrames(unittest.TestCase):
    def pacing_session(self, change=None, sample=100, allocation=3, counter=13, selection=True):
        control = NativeControl()
        control.update = 0
        request = control.request
        def observed(command):
            result = request(command)
            if command['type'] == 'state':
                result['clock']['bios_sample'] = sample
            if command['type'] == 'scripts':
                total = (sample + allocation + 2**31) % 2**32 - 2**31
                remainder = None if counter == 0 or (total == -2**31 and counter == -1) else (abs(total) % abs(counter)) * (-1 if total < 0 else 1)
                phase = dict(ordinary_update=control.update, cached_bios_sample=sample,
                             allocation_index=allocation, pacing_sum=total,
                             defence_counter=counter, counter_remainder=remainder,
                             pacing_remainder=remainder,
                             selection=dict(pacing_sum=total, defence_pacing=counter) if selection else None)
                result = dict(actors=[dict(name='opponent', combat_pacing=phase)], total=1)
                if change:
                    change(result, control)
                result['total'] = len(result['actors'])
            return result
        control.request = observed
        session = pair.NativeFrameSession(control, lambda *_: {}, frame_scales=[202],
                    bios_ticks=[0], cached_bios_samples=[sample, sample],
                    combat_pacing=[[], [dict(actor='opponent', allocation_index=allocation, cached_bios_sample=sample, selection=selection)]])
        return session, control

    def test_combat_phase_accepts_actual_current_selection_and_retains_evidence(self):
        session, control = self.pacing_session()
        session.advance_to(1)
        session.input('ArrowDown', 'press')
        evidence = session.execution_evidence()
        self.assertEqual(evidence['combat_observations'][0]['observation']['ordinary_update'], 1)
        self.assertEqual(len(evidence['combat_observations']), 1)
        self.assertEqual(control.keys, [(1, 'ArrowDown', 'press')])
        for sample, allocation, counter in [(2**31 - 1, 1, 29), (2**31, 0, -1), (100, 0, 0)]:
            with self.subTest(sample=sample, counter=counter):
                session, _ = self.pacing_session(sample=sample, allocation=allocation, counter=counter)
                session.advance_to(1)
        session, _ = self.pacing_session(selection=False)
        session.advance_to(1)  # Current combat entry is distinct from selection.

    def test_combat_decision_cache_is_distinct_from_later_boundary_sample(self):
        def earlier_decision(rows, _):
            phase = rows['actors'][0]['combat_pacing']
            phase.update(cached_bios_sample=99, pacing_sum=102,
                         counter_remainder=11, pacing_remainder=11)
            phase['selection']['pacing_sum'] = 102
        session, _ = self.pacing_session(earlier_decision)
        session.combat_pacing[1][0]['cached_bios_sample'] = 99
        session.advance_to(1)
        self.assertEqual(session.execution_evidence()['combat_observations'][0]['observation']['cached_bios_sample'], 99)
        self.assertEqual(session.execution_evidence()['clocks'][-1]['cached_bios_sample'], 100)

    def test_combat_phase_rejects_stale_missing_and_misaligned_before_input(self):
        changes = [
            lambda rows, _: rows['actors'][0].pop('combat_pacing'),
            lambda rows, _: rows['actors'][0]['combat_pacing'].update(ordinary_update=0),
            lambda rows, _: rows['actors'][0]['combat_pacing'].update(cached_bios_sample=101),
            lambda rows, _: rows['actors'][0]['combat_pacing'].update(allocation_index=4),
            lambda rows, _: rows['actors'][0]['combat_pacing'].update(pacing_sum=104),
            lambda rows, _: rows['actors'][0]['combat_pacing'].update(counter_remainder=None),
            lambda rows, _: rows['actors'][0]['combat_pacing'].update(pacing_remainder=None),
            lambda rows, _: rows['actors'][0]['combat_pacing']['selection'].update(pacing_sum=104),
            lambda rows, _: rows['actors'][0]['combat_pacing'].update(selection=None),
            lambda rows, _: rows['actors'][0]['combat_pacing'].pop('selection'),
            lambda rows, _: rows['actors'].append(copy.deepcopy(rows['actors'][0])),
        ]
        for change in changes:
            with self.subTest(change=change):
                session, control = self.pacing_session(change)
                with self.assertRaises(RuntimeError):
                    session.advance_to(1)
                with self.assertRaises(RuntimeError):
                    session.input('ArrowDown', 'press')
                self.assertEqual(control.keys, [])

    def test_combat_phase_rejects_skipped_boundary_without_indexing_past_tape(self):
        session, control = self.pacing_session()
        control.stride = 2
        with self.assertRaisesRegex(RuntimeError, 'no boundary for frame 2'):
            session.advance_to(1)
        self.assertEqual(control.keys, [])

    def test_combat_fixture_rejects_incomplete_or_ambiguous_contract(self):
        bad = [[], [[], {}], [[], [dict(actor='opponent')]],
               [[], [dict(actor='opponent', allocation_index=1, selection=True)]],
               [[], [dict(actor='opponent', allocation_index=1, cached_bios_sample=True, selection=True)]],
               [[], [dict(actor='opponent', allocation_index=True, cached_bios_sample=100, selection=True)]],
               [[], [dict(actor='opponent', allocation_index=1, cached_bios_sample=100, selection=None)]],
               [[], [dict(actor='opponent', allocation_index=1, cached_bios_sample=100, selection=True)] * 2]]
        for fixture in bad:
            with self.subTest(fixture=fixture), self.assertRaises(ValueError):
                pair.NativeFrameSession(NativeControl(), lambda *_: {}, frame_scales=[202],
                                        cached_bios_samples=[100, 100], combat_pacing=fixture)

    def test_constructor_phase_rejection_retains_actual_sample_without_projection(self):
        control = NativeControl()
        request = control.request
        def observed(command):
            value = request(command)
            if command['type'] == 'state':
                value['clock']['bios_sample'] = 100
            return value
        control.request = observed
        projected = []
        with self.assertRaises(pair.NativePhaseError) as raised:
            pair.NativeFrameSession(control, lambda *_: projected.append(True),
                                    frame_scales=[202], cached_bios_samples=[101, 101])
        evidence = raised.exception.evidence
        self.assertEqual(evidence['clock']['bios_sample'], 100)
        self.assertEqual(evidence['expected_bios_sample'], 101)
        self.assertEqual(evidence['ordinary_update'], 50)
        self.assertEqual(projected, [])
        self.assertEqual(control.scales, [])
        self.assertEqual(control.keys, [])
        control.update = 99
        self.assertEqual(evidence['ordinary_update'], 50)

    def test_rejected_actor_phase_preserves_actual_stale_decision(self):
        def stale(rows, _):
            rows['actors'][0]['combat_pacing']['ordinary_update'] = 0
        session, control = self.pacing_session(stale)
        with self.assertRaises(pair.NativePhaseError) as raised:
            session.advance_to(1)
        evidence = raised.exception.evidence
        self.assertEqual(evidence['ordinary_update'], 1)
        self.assertEqual(evidence['combat_pacing'][0]['observation']['ordinary_update'], 0)
        self.assertEqual(evidence['clock']['bios_sample'], 100)
        self.assertEqual(control.keys, [])
        session.current[1]['actors'][0]['combat_pacing']['ordinary_update'] = 99
        self.assertEqual(evidence['combat_pacing'][0]['observation']['ordinary_update'], 0)

    def test_pair_serializes_phase_failure_when_backend_construction_rejects(self):
        @contextmanager
        def original(*_):
            yield Session({0: state()}, [], 'original')
        @contextmanager
        def native(*_):
            yield pair.NativeFrameSession(NativeControl(), lambda *_: {}, frame_scales=[202],
                                          cached_bios_samples=[100, 100])
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(SEGMENT, original, native, Path(directory))
        saved = json.loads(json.dumps(report))
        self.assertEqual(saved['status'], 'blocked')
        self.assertEqual(saved['phase_rejection']['expected_bios_sample'], 100)
        self.assertNotIn('bios_sample', saved['phase_rejection']['clock'])
        self.assertEqual(saved['input_attempts']['native'], [])

    def test_absolute_bios_phase_rejects_equal_delta_before_projection(self):
        control = NativeControl()
        original = control.request
        sample, increment = [100], [2]
        def request(command):
            if command['type'] == 'replay_frame': sample[0] += increment[0]
            result = original(command)
            if command['type'] == 'state': result['clock']['bios_sample'] = sample[0]
            return result
        control.request = request
        session = pair.NativeFrameSession(control, lambda *_: {}, frame_scales=[256],
                                           bios_ticks=[1], cached_bios_samples=[100, 101])
        with self.assertRaisesRegex(RuntimeError, 'cached BIOS phase'):
            session.advance_to(1)
        self.assertEqual(control.bios_ticks, [1])
        with self.assertRaisesRegex(RuntimeError, 'cached BIOS phase'): session.snapshot()
        sample[0], increment[0], control.update = 100, 1, 0
        positive = pair.NativeFrameSession(control, lambda *_: {}, frame_scales=[256],
                                           bios_ticks=[1], cached_bios_samples=[100, 101])
        positive.advance_to(1)
        self.assertEqual(positive.snapshot()['frame'], 1)
        self.assertEqual(positive.execution_evidence()['cached_bios_samples'], [100, 101])
        # Same elapsed delta at another absolute origin has a different pacing remainder.
        sample[0] = 101
        with self.assertRaisesRegex(RuntimeError, 'cached BIOS phase'):
            pair.NativeFrameSession(control, lambda *_: {}, frame_scales=[256],
                                    bios_ticks=[1], cached_bios_samples=[100, 101])
        self.assertNotEqual((100 + 3) % 13, (101 + 3) % 13)

    def test_absolute_bios_requires_complete_observed_phase(self):
        for tape in ([1], [1, True], [-1, 0]):
            with self.assertRaises(ValueError):
                pair.NativeFrameSession(NativeControl(), lambda *_: {}, frame_scales=[256],
                                        cached_bios_samples=tape)
        with self.assertRaisesRegex(RuntimeError, 'sample unavailable'):
            pair.NativeFrameSession(NativeControl(), lambda *_: {}, frame_scales=[256],
                                    cached_bios_samples=[100, 101])

    def test_entry_provenance_is_observed_once_and_copied(self):
        control = NativeControl()
        control.update = 0
        request = control.request
        entry = {'route': 'retail_introduction', 'scene': 'START', 'world': 0, 'marker': 0}
        def observed(command):
            value = request(command)
            if command['type'] == 'state':
                value['entry'] = copy.deepcopy(entry)
            return value
        control.request = observed
        session = pair.NativeFrameSession(control, lambda *_: {}, expected_initial_update=0)
        self.assertEqual(session.acquisition_evidence()['entry'], entry)
        entry['route'] = 'skipped_introduction'
        session.advance_to(1)
        retained = session.acquisition_evidence()
        self.assertEqual(retained['entry']['route'], 'retail_introduction')
        retained['entry']['marker'] = 99
        self.assertEqual(session.acquisition_evidence()['entry']['marker'], 0)
        self.assertEqual(retained['observations'][-1]['entry']['route'], 'skipped_introduction')

    def test_torn_entry_provenance_rejects_before_replay(self):
        control = NativeControl()
        request = control.request
        reads = 0
        def torn(command):
            nonlocal reads
            value = request(command)
            if command['type'] == 'state':
                reads += 1
                value['entry'] = {'marker': reads}
            return value
        control.request = torn
        with self.assertRaisesRegex(RuntimeError, 'entry provenance changed'):
            pair.NativeFrameSession(control, lambda *_: {})
        self.assertEqual(control.scales, [])

    def test_missing_entry_is_not_invented_from_expected_initial_update(self):
        control = NativeControl()
        control.update = 0
        session = pair.NativeFrameSession(control, lambda *_: {}, expected_initial_update=0)
        self.assertIsNone(session.acquisition_evidence()['entry'])

    def test_acquisition_rejects_uncontrolled_bootstrap_instead_of_rebasing_it_to_zero(self):
        control = NativeControl()
        for invalid in (True, -1, 0.5):
            with self.assertRaisesRegex(ValueError, 'nonnegative integer'):
                pair.NativeFrameSession(None, None, expected_initial_update=invalid)
        with self.assertRaisesRegex(ValueError, 'expected initial update 0, observed 50'):
            pair.NativeFrameSession(control, lambda *_: {}, expected_initial_update=0)
        self.assertEqual(control.scales, [])
        self.assertEqual(control.update, 50)
        session = pair.NativeFrameSession(control, lambda *_: {}, expected_initial_update=50)
        self.assertEqual(session.acquisition_evidence()['initial_update'], 50)
        self.assertEqual(session.snapshot()['frame'], 0)

    def test_acquisition_retains_absolute_rng_clock_boundaries_and_forwards_host_time(self):
        control = NativeControl()
        control.update = 0
        commands, request = [], control.request
        def measured(command):
            commands.append(copy.deepcopy(command))
            value = request(command)
            if command['type'] == 'state':
                value.update(random_state=100 + control.update, startup={'phase': 'loaded'})
                value['scripts']['world'] = 0
            return value
        control.request = measured
        session = pair.NativeFrameSession(control, lambda *_: {}, expected_initial_update=0,
                                         frame_scales=[122, 99], bios_ticks=[0, 1], host_nanos=[0, 123])
        before = session.acquisition_evidence()
        session.snapshot()
        self.assertEqual(session.acquisition_evidence(), before)
        session.advance_to(2)
        evidence = session.acquisition_evidence()
        self.assertEqual([row['ordinary_updates'] for row in evidence['observations']], [0, 1, 2])
        self.assertEqual([row['random_state'] for row in evidence['observations']], [100, 101, 102])
        self.assertEqual([row['world'] for row in evidence['observations']], [0, 0, 0])
        self.assertTrue(all(isinstance(row['clock'], dict) for row in evidence['observations']))
        self.assertEqual([row for row in commands if row['type'] == 'replay_frame'], [
            {'type': 'replay_frame', 'frame_scale': 122, 'elapsed_bios_ticks': 0, 'elapsed_host_nanos': 0},
            {'type': 'replay_frame', 'frame_scale': 99, 'elapsed_bios_ticks': 1, 'elapsed_host_nanos': 123}])
        evidence['observations'].clear()
        self.assertEqual(len(session.acquisition_evidence()['observations']), 3)
        self.assertEqual(session.execution_evidence()['host_nanos'], [0, 123])

    def test_invalid_host_time_tape_is_rejected_before_control_access(self):
        for tape in ([True], [-1], [2**64], [], [1, 2]):
            with self.subTest(tape=tape), self.assertRaisesRegex(ValueError, 'nanosecond interval'):
                pair.NativeFrameSession(None, None, frame_scales=[122], host_nanos=tape)

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

    def test_input_rejects_missing_or_wrong_key_action_acknowledgement(self):
        for reply in ({}, {'key': 'ArrowUp', 'action': 'press'},
                      {'key': 'ControlLeft', 'action': 'release'}):
            with self.subTest(reply=reply):
                control = NativeControl()
                request = control.request
                def wrong(command):
                    value = request(command)
                    return reply if command['type'] == 'input' else value
                control.request = wrong
                session = pair.NativeFrameSession(control, lambda *_: {})
                with self.assertRaisesRegex(RuntimeError, 'input acknowledgement'):
                    session.input('ControlLeft', 'press')
                self.assertEqual(control.update, 50)
                self.assertEqual(control.events, [])

    def test_input_cannot_change_clock_without_an_update(self):
        control = NativeControl()
        request = control.request
        def moved(command):
            value = request(command)
            if command['type'] == 'state' and control.keys:
                value['clock']['continuation']['step']['frame_scale'] = 7
            return value
        control.request = moved
        session = pair.NativeFrameSession(control, lambda *_: {})
        with self.assertRaisesRegex(RuntimeError, 'clock changed during key acknowledgement'):
            session.input('ControlLeft', 'press')
        self.assertEqual(control.update, 50)
        self.assertEqual(control.events, [])

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
        self.assertEqual([row['frame'] for row in session.input_observations], [0, 0, 1])
        self.assertEqual(session.input_observations[-1]['acknowledgement'],
                         dict(key='ControlLeft', action='release'))

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


class NativeEvidence(unittest.TestCase):
    def compare(self, failure=None):
        control = NativeControl()
        request = control.request
        def respond(command):
            value = request(command)
            if failure == 'clock' and command['type'] == 'state' and control.update >= 52:
                value['clock']['continuation']['step']['frame_scale'] = 7
            if failure == 'input' and command['type'] == 'input' and command['action'] == 'release':
                return {'key': 'WrongKey', 'action': 'release'}
            return value
        control.request = respond
        native = pair.NativeFrameSession(control, lambda *_: state(),
                                         frame_scales=[122, 99, 0], bios_ticks=[0, 2, 1])
        @contextmanager
        def original(*_):
            yield Session({0: state()}, [], 'original')
        @contextmanager
        def backend(*_):
            yield native
        with tempfile.TemporaryDirectory() as directory:
            report = pair.run_pair(SEGMENT, original, backend, Path(directory))
        return report, native

    def test_successful_report_serializes_requested_and_observed_native_tapes(self):
        report, native = self.compare()
        self.assertEqual(report['status'], 'match', report)
        saved = json.loads(json.dumps(report))['native_execution']
        self.assertEqual(saved['frame_scales'], [122, 99, 0])
        self.assertEqual(saved['bios_ticks'], [0, 2, 1])
        self.assertEqual([row['elapsed_scale'] for row in saved['clocks']], [0, 122, 221, 221])
        self.assertEqual([row['frame'] for row in saved['inputs']], [0, 2])
        self.assertEqual(saved['inputs'][-1]['acknowledgement'],
                         dict(key='ControlLeft', action='release'))
        native.clock_observations[-1]['continuation']['step']['frame_scale'] = 700
        native.input_observations.clear()
        self.assertEqual(report['native_execution'], saved)

    def test_diagnostic_session_does_not_invent_recorded_clock_or_bios_tapes(self):
        session = pair.NativeFrameSession(NativeControl(), lambda *_: state())
        session.input('ControlLeft', 'press')
        evidence = session.execution_evidence()
        self.assertIsNone(evidence['frame_scales'])
        self.assertIsNone(evidence['bios_ticks'])
        self.assertEqual(evidence['clocks'], [])
        self.assertEqual(len(evidence['inputs']), 1)

    def test_rejected_consumed_scale_retains_partial_clock_and_input_evidence(self):
        report, _ = self.compare('clock')
        self.assertEqual(report['status'], 'blocked', report)
        self.assertIn('consumed clock scale 7, requested 99', report['reason'])
        evidence = report['native_execution']
        self.assertEqual([row['frame'] for row in evidence['clocks']], [0, 1, 2])
        self.assertEqual(evidence['clocks'][-1]['requested_scale'], 99)
        self.assertEqual(evidence['clocks'][-1]['continuation']['step']['frame_scale'], 7)
        self.assertEqual(len(evidence['inputs']), 1)

    def test_rejected_input_retains_only_accepted_acknowledgements(self):
        report, _ = self.compare('input')
        self.assertEqual(report['status'], 'blocked', report)
        self.assertIn('input acknowledgement differs', report['reason'])
        evidence = report['native_execution']
        self.assertEqual([row['frame'] for row in evidence['clocks']], [0, 1, 2])
        self.assertEqual([(row['key'], row['action']) for row in evidence['inputs']],
                         [('ControlLeft', 'press')])


class VerifiedArtifact(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.binary = Path(sys.executable).resolve()
        self.head, self.tree = 'a' * 40, 'b' * 40
        self.receipt = dict(commit=self.head, run=7, artifact=11,
                            archive_sha256='c' * 64, executable_sha256=self.digest(self.binary),
                            checkout_commit='d' * 40, checkout_tree=self.tree, checkout_job=12)
        self.receipt_path = self.root / 'receipt.json'
        self.calls = []

    @staticmethod
    def digest(path):
        with path.open('rb') as stream:
            return hashlib.file_digest(stream, 'sha256').hexdigest()

    def artifact(self):
        self.receipt_path.write_text(json.dumps(self.receipt))
        return pair.NativeArtifact(self.binary, self.receipt_path, self.head, self.tree)

    @contextmanager
    def original(self, *_):
        self.calls.append('original')
        yield Session({0: state()}, [], 'original')

    @contextmanager
    def native(self, *_, executable):
        self.calls.append('native')
        process = executable.launch(['-c', 'import time; time.sleep(30)'])
        try:
            session = Session({0: state()}, [], 'native')
            session.process = process
            yield session
        finally:
            process.terminate()
            process.wait(timeout=5)

    def test_pair_launches_bound_executable_and_records_identity(self):
        report = pair.run_pair(SEGMENT, self.original, self.native, self.root,
                               native_artifact=self.artifact())
        self.assertEqual(report['status'], 'match', report)
        self.assertEqual(self.calls, ['original', 'native', 'original', 'native'])
        self.assertEqual(report['native_artifact']['checkout_tree'], self.tree)
        self.assertEqual(report['native_artifact']['executable_sha256'], self.digest(self.binary))

    def test_pair_retains_executable_when_original_backend_removes_path(self):
        import shutil
        self.binary = self.root / 'retained-python'
        shutil.copy2(sys.executable, self.binary)
        original = self.original
        @contextmanager
        def removing_original(*arguments):
            self.binary.unlink(missing_ok=True)
            with original(*arguments) as session:
                yield session
        report = pair.run_pair(SEGMENT, removing_original, self.native, self.root,
                               native_artifact=self.artifact())
        self.assertEqual(report['status'], 'match', report)
        self.assertEqual(self.calls, ['original', 'native', 'original', 'native'])
        self.assertFalse(self.binary.exists())

    def test_head_tree_or_bytes_mismatch_prevents_both_backends(self):
        for field in ['commit', 'checkout_tree', 'executable_sha256']:
            original = self.receipt[field]
            self.receipt[field] = '0' * len(original)
            report = pair.run_pair(SEGMENT, self.original, self.native, self.root,
                                   native_artifact=self.artifact())
            self.assertEqual(report['status'], 'blocked')
            self.assertEqual(self.calls, [])
            self.assertIn('differs', report['reason'])
            self.receipt[field] = original

    def test_legacy_receipt_cannot_claim_a_source_bound_pair(self):
        del self.receipt['checkout_tree']
        report = pair.run_pair(SEGMENT, self.original, self.native, self.root,
                               native_artifact=self.artifact())
        self.assertEqual(report['status'], 'blocked')
        self.assertEqual(self.calls, [])
        self.assertIn('checkout', report['reason'])

    def test_lease_survives_unlink_for_multiple_real_launches_and_releases(self):
        import shutil
        self.binary = self.root / 'retained-python'
        shutil.copy2(sys.executable, self.binary)
        artifact = self.artifact()
        with artifact.lease():
            self.binary.unlink()
            for _ in range(2):
                process = artifact.launch(['-c', 'print("retained")'], stdout=subprocess.PIPE)
                self.assertEqual(process.communicate(timeout=5)[0], b'retained\n')
                self.assertEqual(process.returncode, 0)
            with self.assertRaisesRegex(ValueError, 'already active'):
                with artifact.lease(): pass
        with self.assertRaises(FileNotFoundError): artifact.launch(['--version'])

    def test_replaced_binary_is_rejected_again_at_launch(self):
        self.binary = self.root / 'cyrus'
        self.binary.write_bytes(b'initial executable')
        self.receipt['executable_sha256'] = self.digest(self.binary)
        artifact = self.artifact()
        artifact.validate()
        self.binary.write_bytes(b'replaced executable')
        with self.assertRaisesRegex(ValueError, 'executable differs'):
            artifact.launch(['--version'])
        self.assertEqual(artifact.processes, [])

    def test_changed_receipt_cannot_switch_identity_during_original_replay(self):
        artifact = self.artifact()
        artifact.validate()
        self.receipt['artifact'] = 999
        self.receipt_path.write_text(json.dumps(self.receipt))
        with self.assertRaisesRegex(ValueError, 'receipt changed'):
            artifact.launch(['--version'])
        self.assertEqual(artifact.processes, [])

    def test_cli_rejects_mismatch_before_original_runtime_preparation(self):
        self.receipt['checkout_tree'] = '0' * 40
        self.artifact()
        arguments = ['compare-segment.py', 'unused.json', '--original-backend', 'unused.py',
                     '--native-backend', 'unused.py', '--out', str(self.root),
                     '--native-executable', str(self.binary), '--native-receipt', str(self.receipt_path),
                     '--native-head', self.head, '--native-source-tree', self.tree]
        with mock.patch.object(sys, 'argv', arguments), \
                mock.patch.object(pair.subprocess, 'run') as prepare, \
                mock.patch('sys.stderr'):
            with self.assertRaises(SystemExit) as result:
                pair.main()
        self.assertEqual(result.exception.code, 2)
        prepare.assert_not_called()

    def test_backend_cannot_ignore_verified_launcher(self):
        @contextmanager
        def bypass(*_, executable):
            yield Session({0: state()}, [], 'native')
        report = pair.run_pair(SEGMENT, self.original, bypass, self.root,
                               native_artifact=self.artifact())
        self.assertEqual(report['status'], 'blocked')
        self.assertIn('verified launcher', report['reason'])


if __name__ == '__main__':
    unittest.main()
