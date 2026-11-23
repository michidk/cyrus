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


class FrameTests(unittest.TestCase):
    def session(self, boundary, emit=lambda raw: None):
        return adapter.FrameSession(boundary, lambda raw: raw['frame'],
                                    lambda raw: dict(player=raw['player']), emit)

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

    def test_reader_returns_frame_and_all_observables_in_one_stopped_batch(self):
        import json
        from types import SimpleNamespace
        boundary = object.__new__(watchpoint.Watchpoint)
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


if __name__ == '__main__':
    unittest.main()
