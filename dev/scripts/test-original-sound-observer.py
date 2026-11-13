#!/usr/bin/env python3
"""Failure controls for stopped sound-service observation, without game layouts."""
import importlib.util
from pathlib import Path
from types import SimpleNamespace
import unittest
import os
import struct
import subprocess
import sys
import time

spec=importlib.util.spec_from_file_location('observer',Path(__file__).with_name('original-sound-observer.py'))
observer=importlib.util.module_from_spec(spec);spec.loader.exec_module(observer)


def event(sequence, kind='play'):
    return dict(sequence=sequence,kind=kind,slot=0,generation=1,
        snapshot=dict(frame=1,game_seconds=.25,owner='player',animation='animation',group=91,
                      mixer_generation=[1]+[0]*15))


class Stream(unittest.TestCase):
    def run_stream(self, snapshots, limit=4):
        rows=iter(snapshots);published=[];remaining=[len(snapshots)]
        def capture(hits):
            self.assertEqual(hits,[0,1]);remaining[0]-=1;return next(rows)
        count=observer.record(SimpleNamespace(capture=capture),SimpleNamespace(next=lambda deadline:[0,1]),
            published.append,lambda:remaining[0]==0,1,limit)
        return count,published

    def test_complete_ordered_service_stream_survives_empty_clock_samples(self):
        snapshots=[dict(events=[],overflow=False),dict(events=[event(1),event(2,'remember')],overflow=False),dict(events=[event(3,'stop')],overflow=False)]
        self.assertEqual(self.run_stream(snapshots),(3,snapshots))

    def test_loss_overflow_and_absent_event_stream_reject(self):
        for snapshot in [dict(overflow=False),dict(events=[],overflow=True),dict(events=[]),dict(events=[event(2)],overflow=False),dict(events=[event(1),event(1)],overflow=False)]:
            with self.subTest(snapshot=snapshot),self.assertRaises(ValueError):self.run_stream([snapshot])

    def test_capacity_is_checked_before_publishing_partial_batch(self):
        with self.assertRaisesRegex(ValueError,'overflow'):
            self.run_stream([dict(events=[event(1),event(2)],overflow=False)],1)

    def test_missing_atomic_snapshot_or_generation_is_rejected(self):
        for change in [dict(snapshot={}),dict(generation=0),dict(generation=2),dict(slot=16),dict(kind='guessed_play')]:
            value=event(1);value.update(change)
            with self.subTest(change=change),self.assertRaises(ValueError):
                self.run_stream([dict(events=[value],overflow=False)])

    def test_hardware_watch_requires_stopped_aligned_boundary(self):
        watch=observer.WriteWatches(1);calls=[];watch.trace=lambda *args:calls.append(args)
        with self.assertRaises(ValueError):watch.install([(16,4)])
        watch.stopped=True
        for invalid in [[],[(17,4)],[(16,3)],[(16,1)]*5]:
            with self.assertRaises(ValueError):watch.install(invalid)
        watch.install([(16,2),(32,1)])
        self.assertEqual(calls[-1],(6,904,1|4|(5<<16)|(1<<20)))

    def test_detach_clears_watch_registers_before_resuming(self):
        watch=observer.WriteWatches(1);calls=[];watch.trace=lambda *args:calls.append(args)
        watch.attached=True;watch.stopped=True;watch.close()
        self.assertEqual(calls,[(6,904,0),(17,)])


class Hardware(unittest.TestCase):
    def test_real_write_trap_preserves_stopped_value_and_detaches(self):
        child=subprocess.Popen([sys.executable,'-c',
            'import ctypes,sys; x=ctypes.c_uint32(0); print(ctypes.addressof(x),flush=True); sys.stdin.readline(); x.value=123; print("done",flush=True)'],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        watch=observer.WriteWatches(child.pid)
        try:
            address=int(child.stdout.readline())
            watch.attach();watch.install([(address,4)])
            child.stdin.write('go\n');child.stdin.flush()
            self.assertEqual(watch.next(time.monotonic()+3),[0])
            with open(f'/proc/{child.pid}/mem','rb',0) as memory:
                memory.seek(address)
                self.assertEqual(struct.unpack('<I',memory.read(4))[0],123)
            watch.close()
            self.assertEqual(child.stdout.readline().strip(),'done')
            self.assertEqual(child.wait(timeout=3),0)
        finally:
            watch.close()
            if child.poll() is None:child.kill();child.wait()
            child.stdin.close();child.stdout.close()

    def test_each_short_lived_word_write_is_observed_before_the_next(self):
        child=subprocess.Popen([sys.executable,'-c',
            'import ctypes,sys; x=ctypes.c_uint16(0); print(ctypes.addressof(x),flush=True); sys.stdin.readline(); '
            '[(setattr(x,"value",i)) for i in range(1,17)]; print("done",flush=True)'],
            stdin=subprocess.PIPE,stdout=subprocess.PIPE,text=True)
        watch=observer.WriteWatches(child.pid)
        try:
            address=int(child.stdout.readline())
            watch.attach();watch.install([(address,2)])
            child.stdin.write('go\n');child.stdin.flush()
            with open(f'/proc/{child.pid}/mem','rb',0) as memory:
                for expected in range(1,17):
                    self.assertEqual(watch.next(time.monotonic()+3),[0])
                    memory.seek(address)
                    self.assertEqual(struct.unpack('<H',memory.read(2))[0],expected)
            watch.close()
            self.assertEqual(child.stdout.readline().strip(),'done')
            self.assertEqual(child.wait(timeout=3),0)
        finally:
            watch.close()
            if child.poll() is None:child.kill();child.wait()
            child.stdin.close();child.stdout.close()


if __name__=='__main__':
    suite=unittest.TestSuite()
    suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(Stream))
    if os.environ.get('CYRUS_OBSERVER_HARDWARE')=='1':
        suite.addTests(unittest.defaultTestLoader.loadTestsFromTestCase(Hardware))
    else:
        print('Hardware control requires Linux x86-64 ptrace permission; set CYRUS_OBSERVER_HARDWARE=1 in the original test container.')
    sys.exit(not unittest.TextTestRunner().run(suite).wasSuccessful())
