#!/usr/bin/env python3
"""Original acquisition handoff contracts with synthetic files and real clock validation."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


recorder = load('original-update-recorder')


class Artifact:
    def __init__(self):
        self.receipt = dict(commit='a' * 40, checkout_tree='b' * 40, executable_sha256='c' * 64)

    def validate(self):
        return copy.deepcopy(self.receipt)

    def check_session(self, session):
        if getattr(session, 'launcher', None) is not self:
            raise ValueError('session did not use verified launcher')


class AcquisitionTests(unittest.TestCase):
    def setUp(self):
        import original_acquisition as acquisition
        self.module = acquisition
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        root = Path(self.temporary.name)
        self.executable, self.save, self.path = (root / name for name in ('original', 'save', 'tape.json'))
        self.executable.write_bytes(b'synthetic original executable')
        self.save.write_bytes(b'synthetic unchanged save')
        self.artifact = Artifact()
        state, ordinal = 1, 0
        def section(phase, counter):
            nonlocal state, ordinal
            previous = state
            state = (state * 1103515245 + 12345) % 2**32
            ordinal += 1
            return dict(random_state=state, writes=[dict(draw=ordinal, previous=previous,
                state=state, phase=phase, context=dict(counter=counter, movement=256))])
        loading = section('loading', 0)
        self.tape = dict(version=1, original=dict(
            executable_sha256=hashlib.sha256(self.executable.read_bytes()).hexdigest(),
            save_sha256=hashlib.sha256(self.save.read_bytes()).hexdigest()),
            entry=dict(route='retail_introduction', scene='START', world=0, marker=0),
            initial_random_state=1, loading=loading,
            loading_artwork=dict(frames=3, selected_frame=((state >> 16) & 32767) % 3, draw=1),
            updates=[], book={}, reload={})
        for update in (1, 2):
            frame = section('ordinary', update)
            frame.update(frame_scale=256, bios_observation=dict(update=update, consumed_frame_scale=256, bios_ticks=10,
                clock_publication=dict(bios_sample=10), bios_measurement=dict(
                    phase='before_publication', sample=10, baseline=9,
                    remainder_before=2, remainder_after=3)))
            self.tape['updates'].append(frame)
        self.tape['book'] = dict(completed_updates=2, counter=3, random_state=state)
        self.tape['reload'] = section('reload', 3) | dict(world=30)

    def bind(self):
        self.path.write_text(json.dumps(self.tape))
        return self.module.OriginalAcquisition(self.path, self.executable, self.save,
                                              self.artifact)

    def native(self, acquisition):
        from types import SimpleNamespace
        inputs = acquisition.replay_inputs()
        observations, clocks, elapsed = [], [], 0
        for update, random in enumerate(inputs['random_states']):
            scale = 0 if update == 0 else inputs['frame_scales'][update - 1]
            elapsed += scale
            continuation = dict(step=dict(frame_scale=scale))
            observations.append(dict(ordinary_updates=update, world=0, random_state=random,
                                     clock=dict(continuation=copy.deepcopy(continuation))))
            clocks.append(dict(frame=update, elapsed_scale=elapsed, continuation=continuation))
        native = SimpleNamespace(launcher=self.artifact,
            acquisition=dict(entry=inputs['entry'], expected_initial_update=0, initial_update=0, observations=observations),
            execution=dict(frame_scales=inputs['frame_scales'], bios_ticks=inputs['bios_ticks'], clocks=clocks))
        native.acquisition_evidence = lambda: copy.deepcopy(native.acquisition)
        native.execution_evidence = lambda: copy.deepcopy(native.execution)
        return native

    def test_native_acquisition_requires_verified_launch_and_all_measured_bodies(self):
        acquisition = self.bind()
        with self.assertRaisesRegex(ValueError, 'admitted native'):
            acquisition.require_native_admission()
        native = self.native(acquisition)
        proof = acquisition.admit_native(native)
        acquisition.require_native_admission()
        proof['acquisition']['observations'].clear()
        self.assertEqual(len(acquisition.evidence()['native_admission']['acquisition']['observations']), 3)

    def test_native_missing_changed_or_uncontrolled_history_clears_admission(self):
        changes = [lambda n: setattr(n, 'launcher', None),
            lambda n: n.acquisition.pop('entry'),
            lambda n: n.acquisition['entry'].update(route='direct'),
            lambda n: n.acquisition.update(expected_initial_update=None),
            lambda n: n.acquisition.update(initial_update=2),
            lambda n: n.acquisition['observations'].pop(),
            lambda n: n.acquisition['observations'][0].update(random_state=0),
            lambda n: n.acquisition['observations'][1].update(ordinary_updates=2),
            lambda n: n.acquisition['observations'][1].update(world=1),
            lambda n: n.execution.update(bios_ticks=None),
            lambda n: n.execution.update(bios_ticks=[9, 9]),
            lambda n: n.execution['clocks'][1].update(elapsed_scale=1),
            lambda n: n.execution['clocks'][1]['continuation']['step'].update(frame_scale=0)]
        for change in changes:
            acquisition = self.bind()
            native = self.native(acquisition)
            acquisition.admit_native(native)
            change(native)
            with self.subTest(change=change), self.assertRaises(ValueError):
                acquisition.admit_native(native)
            with self.assertRaises(ValueError):
                acquisition.require_native_admission()

    def test_real_native_adapter_requires_independently_observed_entry(self):
        pair = load('compare-segment')
        acquisition = self.bind()
        baseline = self.native(acquisition)
        observations = baseline.acquisition['observations']
        class Control:
            def __init__(self, entry):
                self.entry, self.update = entry, 0
            def request(self, command):
                if command['type'] == 'state':
                    row = observations[self.update]
                    return dict(scripts=dict(ordinary_updates=self.update, world=row['world'], failed=0),
                                random_state=row['random_state'], clock=copy.deepcopy(row['clock']),
                                entry=copy.deepcopy(self.entry))
                if command['type'] == 'scripts':
                    return dict(actors=[], total=0)
                if command['type'] == 'replay_frame':
                    self.update += 1
                    return {}
                raise AssertionError(command)
        measured = dict(route='retail_introduction', scene='START', world=0, marker=0)
        for entry in (measured, None, dict(measured, route='skipped_introduction'),
                      dict(measured, marker=1), dict(measured, world=30), dict(measured, scene='ISLAND')):
            with self.subTest(entry=entry):
                control = Control(entry)
                session = pair.NativeFrameSession(control, lambda *_: {}, expected_initial_update=0,
                    frame_scales=baseline.execution['frame_scales'], bios_ticks=baseline.execution['bios_ticks'])
                session.launcher = self.artifact
                session.advance_to(2)
                if entry == measured:
                    proof = acquisition.admit_native(session)
                    self.assertEqual(proof['acquisition']['entry'], measured)
                else:
                    with self.assertRaises(ValueError):
                        acquisition.admit_native(session)
                    with self.assertRaises(ValueError):
                        acquisition.require_native_admission()

    def test_bound_handoff_exposes_measured_inputs_and_natural_rng_expectations(self):
        acquisition = self.bind()
        self.assertIs(self.module.next_bios_input, recorder.next_bios_input)
        result = acquisition.replay_inputs()
        self.assertEqual(result['frame_scales'], [256, 256])
        self.assertEqual(result['bios_ticks'], [1, 1])
        self.assertEqual(result['random_states'], [self.tape['loading']['random_state']] +
                         [row['random_state'] for row in self.tape['updates']])
        self.assertEqual(result['loading_artwork'], self.tape['loading_artwork'])
        self.assertEqual(result['native_artifact'], self.artifact.receipt)
        result['frame_scales'][0] = 0
        self.assertEqual(acquisition.replay_inputs()['frame_scales'], [256, 256])
        acquisition.require_entry(dict(world=30, random_state=self.tape['reload']['random_state']))

    def test_missing_or_inconsistent_history_rejects_before_handoff(self):
        changes = [lambda t: t.pop('entry'), lambda t: t['entry'].update(route='direct'),
            lambda t: t.pop('loading'), lambda t: t.update(updates=[]),
            lambda t: t['updates'][0].pop('bios_observation'),
            lambda t: t['updates'][0].update(frame_scale=True),
            lambda t: t['updates'][0]['bios_observation'].update(update=2),
            lambda t: t['updates'][0]['bios_observation'].update(consumed_frame_scale=0),
            lambda t: t['updates'][0]['writes'][0]['context'].update(movement=0),
            lambda t: t['updates'][0]['writes'][0].update(draw=8),
            lambda t: t['updates'][0]['writes'][0]['context'].update(counter=2),
            lambda t: t['updates'][0].update(random_state=0),
            lambda t: t['loading']['writes'].clear(),
            lambda t: t['book'].update(counter=4),
            lambda t: t['book'].update(random_state=0),
            lambda t: t['reload']['writes'][0].update(previous=0),
            lambda t: t['loading_artwork'].update(draw=9),
            lambda t: t['loading_artwork'].update(selected_frame=3)]
        original = copy.deepcopy(self.tape)
        for change in changes:
            self.tape = copy.deepcopy(original)
            change(self.tape)
            with self.subTest(tape=self.tape), self.assertRaises(ValueError):
                self.bind()

    def test_diagnostic_sample_gaps_and_unequal_dual_reads_are_not_bios_inputs(self):
        for change in (dict(remainder_after=4), dict(phase='sample_gap')):
            self.tape['updates'][0]['bios_observation']['bios_measurement'].update(change)
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.bind()

    def test_changed_sources_tape_or_native_receipt_reject_before_use(self):
        for source in ('executable', 'save', 'tape', 'native'):
            with self.subTest(source=source):
                self.setUp()
                acquisition = self.bind()
                if source == 'native':
                    self.artifact.receipt['checkout_tree'] = 'd' * 40
                else:
                    path = dict(executable=self.executable, save=self.save, tape=self.path)[source]
                    path.write_bytes(path.read_bytes() + b' ')
                with self.assertRaises(ValueError):
                    acquisition.replay_inputs()

    def test_original_frame_adapter_requires_history_and_retains_it_before_controls(self):
        frame = load('original-frame-session')
        calls = []
        class Boundary:
            def raw_snapshot(inner):
                return dict(counter=3, world=30, random_state=self.tape['reload']['random_state'])
            def latch_key(inner, key, action):
                calls.append((key, action))
                return inner.raw_snapshot()
        args = (Boundary(), lambda row: row['counter'], lambda row: row)
        with self.assertRaisesRegex(ValueError, 'requires bound'):
            frame.AcquiredFrameSession(*args, acquisition=None)
        acquisition = self.bind()
        with self.assertRaisesRegex(ValueError, 'admitted native'):
            frame.AcquiredFrameSession(*args, acquisition=acquisition)
        acquisition.admit_native(self.native(acquisition))
        session = frame.AcquiredFrameSession(*args, acquisition=acquisition)
        self.assertEqual(session.acquisition_evidence()['native_artifact'], self.artifact.receipt)
        self.path.write_bytes(self.path.read_bytes() + b' ')
        with self.assertRaises(ValueError):
            session.input('ArrowUp', 'press')
        with self.assertRaises(ValueError):
            session.advance_to(1)
        with self.assertRaises(ValueError):
            session.wait_for_world(31, 1)
        self.assertEqual(calls, [])

    def test_restored_world_or_rng_mismatch_is_not_admitted(self):
        acquisition = self.bind()
        for state in (dict(world=29, random_state=self.tape['reload']['random_state']),
                      dict(world=30, random_state=0), dict(world=30)):
            with self.subTest(state=state), self.assertRaises(ValueError):
                acquisition.require_entry(state)


if __name__ == '__main__':
    unittest.main()
