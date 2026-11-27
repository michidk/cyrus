#!/usr/bin/env python3
"""Stopped-frame and key-readback contracts without executable bindings."""
import copy
import importlib.util
import io
from pathlib import Path
import unittest
from unittest.mock import patch


def load(name):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


adapter = load('original-frame-session')
watchpoint = load('original-update-watchpoint')


class Boundary:
    def __init__(self):
        self.raw = dict(frame=80, keys={'ControlLeft': 0}, player={'health': 100})
        self.calls = []
        self.stride = 1

    def raw_snapshot(self):
        self.calls.append(('read', self.raw['frame']))
        return copy.deepcopy(self.raw)

    def next_boundary(self, timeout):
        self.raw['frame'] += self.stride

    def latch_key(self, key, action):
        self.calls.append(('key', self.raw['frame'], key, action))
        self.raw['keys'][key] = int(action == 'press')
        return self.raw_snapshot()


class AcquisitionTests(unittest.TestCase):
    def acquire(self, frames, target=3):
        boundary = Boundary()
        rows = iter(dict(frame=f, random_state=i) for i, f in enumerate(frames))
        boundary.raw = next(rows)
        boundary.next_boundary = lambda timeout: setattr(boundary, 'raw', next(rows))
        history = []
        result = adapter.acquire_entry(boundary, lambda raw: raw['frame'], target,
                                       emit=history.append, max_observations=10)
        return result, history

    def test_loader_zero_writes_are_retained_before_consecutive_ordinary_starts(self):
        result, history = self.acquire([0, 0, 0, 1, 2, 3])
        self.assertEqual(result['frame'], 3)
        self.assertEqual([r['frame'] for r in history], [0, 0, 0, 1, 2, 3])
        self.assertEqual([r['random_state'] for r in history], list(range(6)))

    def test_late_attach_gaps_duplicates_and_resets_fail(self):
        for frames in ([1], [0, 2], [0, 1, 1], [0, 1, 0], [0, 1, 3]):
            with self.subTest(frames=frames), self.assertRaises(ValueError):
                self.acquire(frames)

    def test_zero_is_loader_only_and_not_an_admissible_target(self):
        with self.assertRaises(ValueError):
            self.acquire([0], target=0)


class FrameTests(unittest.TestCase):
    def session(self, boundary, emit=lambda raw: None):
        return adapter.FrameSession(boundary, lambda raw: raw['frame'],
                                    lambda raw: dict(player=raw['player']), emit)

    def test_attach_error_names_operation_target_and_same_runtime_preflight(self):
        import types
        boundary = object.__new__(watchpoint.Watchpoint)
        boundary.pid = 123
        boundary.libc = types.SimpleNamespace(ptrace=lambda *args: -1)
        with patch.object(watchpoint.ctypes, 'get_errno', return_value=13):
            with self.assertRaises(OSError) as raised:
                boundary.ptrace(16)
        self.assertEqual(raised.exception.errno, 13)
        self.assertIn('PTRACE_ATTACH', str(raised.exception))
        self.assertIn('pid 123', str(raised.exception))
        self.assertIn('same runtime', str(raised.exception))

    def test_key_is_acknowledged_before_advancing_and_all_frames_are_batched(self):
        boundary = Boundary()
        rows = []
        session = self.session(boundary, rows.append)
        self.assertEqual(session.input('ControlLeft', 'press'), 0)
        session.advance_to(3)
        self.assertEqual(session.input('ControlLeft', 'release'), 3)
        self.assertEqual([row['frame'] for row in rows], [80, 81, 82, 83])
        self.assertEqual(session.snapshot(), dict(frame=3, player={'health': 100}))
        self.assertLess(boundary.calls.index(('key', 80, 'ControlLeft', 'press')),
                        boundary.calls.index(('read', 81)))

    def test_skipped_duplicate_or_reset_frame_is_rejected_without_interpolation(self):
        for stride in (2, 0, -1):
            boundary = Boundary()
            boundary.stride = stride
            with self.subTest(stride=stride), self.assertRaisesRegex(ValueError, 'frame gap'):
                self.session(boundary).advance_to(1)

    def test_late_key_acknowledgement_is_rejected(self):
        boundary = Boundary()
        session = self.session(boundary)
        latch = boundary.latch_key
        def late(key, action):
            boundary.raw['frame'] += 1
            return latch(key, action)
        boundary.latch_key = late
        with self.assertRaisesRegex(ValueError, 'changed during'):
            session.input('ControlLeft', 'press')

    def test_readback_checks_both_slot_and_frame_without_resuming(self):
        boundary = object.__new__(watchpoint.Watchpoint)
        boundary.observations = {}
        boundary.stopped, boundary.pid, boundary.base = True, 123, 0
        boundary.config = dict(counter='frame', keys={'ControlLeft': dict(guest_offset=4, reader='keys.0')})
        # Synthetic memory models a documented 16-bit key slot, not an executable.
        class Memory(io.BytesIO):
            def close(self):
                pass
        memory = Memory(b'\0' * 8)
        def read():
            return dict(frame=7, keys=[int.from_bytes(memory.getvalue()[4:6], 'little')])
        boundary.raw_snapshot = read
        with patch('builtins.open', return_value=memory):
            self.assertEqual(boundary.latch_key('ControlLeft', 'press')['keys'], [1])
            self.assertEqual(boundary.latch_key('ControlLeft', 'release')['keys'], [0])
            boundary.raw_snapshot = lambda: dict(frame=7, keys=[0])
            with self.assertRaisesRegex(RuntimeError, 'did not acknowledge'):
                boundary.latch_key('ControlLeft', 'press')
            rows = iter([dict(frame=7, keys=[0]), dict(frame=8, keys=[1])])
            boundary.raw_snapshot = lambda: next(rows)
            with self.assertRaisesRegex(RuntimeError, 'frame advanced'):
                boundary.latch_key('ControlLeft', 'press')
        boundary.stopped = False
        with self.assertRaisesRegex(RuntimeError, 'running'):
            boundary.latch_key('ControlLeft', 'press')

    def test_loader_binding_cannot_be_reported_as_an_ordinary_update(self):
        boundary = object.__new__(watchpoint.Watchpoint)
        boundary.observations = {}
        boundary.config = dict(boundary_guest_offset=16)
        boundary.active_boundary = 24
        boundary.raw_snapshot = lambda: self.fail('custom boundary mislabeled as ordinary')
        with self.assertRaisesRegex(RuntimeError, 'ordinary boundary binding'):
            boundary.snapshot()

    def test_pause_stops_only_its_attached_emulator_and_is_idempotent(self):
        boundary = object.__new__(watchpoint.Watchpoint)
        boundary.observations = {}
        boundary.attached, boundary.stopped, boundary.pid = True, False, 123
        waits = []
        def wait(timeout):
            waits.append(timeout)
            boundary.stopped = True
        boundary.wait = wait
        with patch.object(watchpoint.os, 'kill') as kill:
            boundary.pause()
            boundary.pause()
            kill.assert_called_once_with(123, watchpoint.signal.SIGSTOP)
        self.assertEqual(waits, [5])
        boundary.attached = False
        with self.assertRaisesRegex(RuntimeError, 'unattached'):
            boundary.pause()

    def test_boundary_rebinding_is_stopped_and_does_not_resume_execution(self):
        boundary = object.__new__(watchpoint.Watchpoint)
        boundary.observations = {}
        boundary.stopped, boundary.base = True, 4096
        calls = []
        boundary.ptrace = lambda *args: calls.append(args)
        boundary.select_boundary(16)
        self.assertEqual(calls, [(6, 904, 0), (6, 848, 4112),
                                 (6, 896, 0), (6, 904, 1 | (1 << 16) | (3 << 18))])
        for invalid in (-4, 3, None):
            with self.assertRaisesRegex(ValueError, 'aligned'):
                boundary.select_boundary(invalid)
        boundary.stopped = False
        with self.assertRaisesRegex(RuntimeError, 'running'):
            boundary.select_boundary(32)
        self.assertEqual(len(calls), 4)

    def test_reader_returns_frame_and_all_observables_in_one_stopped_batch(self):
        import json
        from types import SimpleNamespace
        boundary = object.__new__(watchpoint.Watchpoint)
        boundary.observations = {}
        calls = []
        raw = dict(frame=8, world=2, actors=[], flags={'test': 1}, prompts=['test'],
                   timing={'consumed': 32}, keys=[1])
        def read():
            calls.append('read')
            print(json.dumps(raw))
        boundary.reader = SimpleNamespace(main=read)
        boundary.stopped = True
        self.assertEqual(boundary.raw_snapshot(), raw)
        self.assertEqual(calls, ['read'])
        boundary.stopped = False
        with self.assertRaisesRegex(RuntimeError, 'running'):
            boundary.raw_snapshot()
        self.assertEqual(calls, ['read'])


class WorldTransitionTests(unittest.TestCase):
    def session(self, worlds):
        boundary = Boundary()
        boundary.raw['world'] = 0
        steps = iter(worlds)
        def advance(timeout):
            world, frame = next(steps)
            boundary.raw.update(world=world, frame=frame)
        boundary.next_boundary = advance
        session = adapter.FrameSession(boundary, lambda raw: raw['frame'],
                                       lambda raw: dict(world=raw['world']))
        return session, boundary

    def test_expected_world_reset_is_explicit_and_next_world_can_resume_at_frame_zero(self):
        session, boundary = self.session([(0, 81), (1, 0), (1, 1)])
        receipt = session.wait_for_world(1, 3)
        self.assertEqual(receipt, dict(start=dict(world=0, frame=0), end=dict(world=1, frame=0), updates=2))
        self.assertEqual(session.input('ControlLeft', 'release'), 0)
        session.advance_to(1)
        self.assertEqual(session.snapshot(), dict(world=1, frame=1))

    def test_other_world_is_rejected_even_if_counter_is_consecutive(self):
        session, _ = self.session([(2, 81)])
        with self.assertRaisesRegex(ValueError, 'expected world 1, observed 2'):
            session.wait_for_world(1, 3)

    def test_same_world_counter_reset_or_skip_still_rejects(self):
        for frame in (0, 80, 82):
            session, _ = self.session([(0, frame)])
            with self.subTest(frame=frame), self.assertRaisesRegex(ValueError, 'frame gap'):
                session.wait_for_world(1, 3)

    def test_missing_world_is_bounded_and_already_arrived_world_is_idempotent(self):
        session, _ = self.session([(0, 81), (0, 82)])
        with self.assertRaisesRegex(TimeoutError, 'world 1.*2 updates'):
            session.wait_for_world(1, 2)
        self.assertEqual(session.wait_for_world(0, 0)['updates'], 0)

    def test_transition_requires_every_dialogue_checkpoint_before_arrival(self):
        session, _ = self.session([(0, 81), (1, 0)])
        checkpoints = [('Brennan first line', lambda state: state['frame'] == 1),
                       ('Brennan reply', lambda state: False)]
        with self.assertRaisesRegex(ValueError, 'missing checkpoint Brennan reply before world 1'):
            session.wait_for_world(1, 3, checkpoints=checkpoints)

    def test_dialogue_checkpoints_are_recorded_in_order_and_later_save_skips_them(self):
        session, _ = self.session([(0, 81), (0, 82), (1, 0)])
        checkpoints = [('first line', lambda state: state['frame'] == 1),
                       ('reply', lambda state: state['frame'] == 2)]
        receipt = session.wait_for_world(1, 3, checkpoints=checkpoints)
        self.assertEqual(receipt['checkpoints'], [dict(name='first line', updates=1), dict(name='reply', updates=2)])
        self.assertEqual(session.wait_for_world(1, 0, checkpoints=checkpoints)['updates'], 0)

    def test_no_world_or_unbounded_wait_rejects_before_advance(self):
        session, boundary = self.session([])
        with self.assertRaisesRegex(ValueError, 'nonnegative'):
            session.wait_for_world(1, -1)
        boundary.raw['world'] = 4294967295
        session.current = boundary.raw_snapshot()
        with self.assertRaisesRegex(ValueError, 'running source world'):
            session.wait_for_world(1, 2)


class ActorObservation(unittest.TestCase):
    def test_camera_exclusion_is_observed_without_claiming_update_admission(self):
        observer = adapter.ActorObserver([55])
        actor = dict(id=55, pointer=100, script_data=200, selected=False, health=250)
        read = lambda pointer: dict(actor)
        self.assertTrue(observer.observe(30, [actor], read)[0]['in_update_list'])
        off_list = observer.observe(30, [], read)[0]
        self.assertFalse(off_list['in_update_list'])
        self.assertEqual(off_list['health'], 250)
        self.assertTrue(observer.observe(30, [actor], read)[0]['in_update_list'])

    def test_recycled_allocation_rejects_instead_of_returning_stale_actor(self):
        observer = adapter.ActorObserver([55])
        actor = dict(id=55, pointer=100, script_data=200, selected=False)
        observer.observe(30, [actor], lambda pointer: dict(actor))
        with self.assertRaisesRegex(ValueError, 'identity changed'):
            observer.observe(30, [], lambda pointer: dict(actor, script_data=201))
        with self.assertRaisesRegex(ValueError, 'identity changed'):
            observer.observe(30, [dict(actor, pointer=101)], lambda pointer: dict(actor))

    def test_world_change_drops_old_bindings_before_any_read(self):
        observer = adapter.ActorObserver([55])
        actor = dict(id=55, pointer=100, script_data=200, selected=False)
        observer.observe(30, [actor], lambda pointer: dict(actor))
        def no_old_read(pointer):
            self.fail('read an allocation from the preceding world')
        self.assertEqual(observer.observe(1, [], no_old_read), [])


class WriteObservationTests(unittest.TestCase):
    def backend(self, stops):
        session = object.__new__(watchpoint.Watchpoint)
        session.base, session.stopped, session.observations = 4096, True, {}
        session.calls, session.registers = [], {}
        session.registers[848 + 7 * 8] = 1 | (1 << 16) | (3 << 18)
        def ptrace(op, offset=0, value=0):
            session.calls.append((op, offset, value))
            if op == 3: return session.registers.get(offset, 0)
            if op == 6: session.registers[offset] = value
        session.ptrace = ptrace
        def wait(timeout):
            import signal
            session.stopped = True
            session.registers[848 + 6 * 8] = stops.pop(0)
            return signal.SIGTRAP
        session.wait = wait
        return session

    def test_mid_update_write_is_observed_without_becoming_an_ordinary_boundary(self):
        session = self.backend([2, 1])
        seen = []
        session.observe_write(1, 33, 1, lambda: seen.append(session.stopped))
        session.next_boundary(2)
        self.assertEqual(seen, [True])
        self.assertEqual(sum(c[0] == 7 for c in session.calls), 2)
        self.assertTrue(session.stopped)

    def test_observer_can_bind_result_write_and_simultaneous_frame_stop_is_retained(self):
        session = self.backend([2, 5])
        seen = []
        session.observe_write(1, 33, 1, lambda: session.observe_write(2, 40, 4, lambda: seen.append('result')))
        session.next_boundary(2)
        self.assertEqual(seen, ['result'])
        self.assertEqual(sum(c[0] == 7 for c in session.calls), 2)

    def test_completed_result_observer_disarms_before_stack_slot_reuse(self):
        session = self.backend([2, 1])
        seen = []
        def result():
            seen.append('complete')
            session.stop_observing_write(1)
        session.observe_write(1, 32, 4, result)
        session.next_boundary(2)
        self.assertEqual(seen, ['complete'])
        self.assertEqual(session.observations, {})
        self.assertEqual(session.registers[848 + 7 * 8], 1 | (1 << 16) | (3 << 18))

    def test_unbound_stop_or_observation_budget_exhaustion_rejects(self):
        session = self.backend([4])
        with self.assertRaisesRegex(RuntimeError, 'unidentified'):
            session.next_boundary(2)
        session = self.backend([2, 2])
        session.observe_write(1, 33, 1, lambda: None)
        with self.assertRaisesRegex(RuntimeError, 'budget'):
            session.next_boundary(2, max_observations=1)
        self.assertTrue(session.stopped)

    def test_invalid_bindings_and_callback_failure_leave_emulator_stopped(self):
        session = self.backend([])
        for slot, offset, size in [(0, 32, 4), (4, 32, 4), (1, 33, 4), (1, -1, 1), (1, 32, 3)]:
            with self.subTest(binding=(slot, offset, size)), self.assertRaises(ValueError):
                session.observe_write(slot, offset, size, lambda: None)
        session = self.backend([2])
        def fail(): raise ValueError('lost actor identity')
        session.observe_write(1, 33, 1, fail)
        with self.assertRaisesRegex(ValueError, 'identity'):
            session.next_boundary(2)
        self.assertTrue(session.stopped)


if __name__ == '__main__':
    unittest.main()
