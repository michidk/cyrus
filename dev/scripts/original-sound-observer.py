#!/usr/bin/env python3
"""Record original sound-service writes while the emulation thread is stopped.

Executable-specific bindings stay outside the tracked tree. A binding module
provides pid(), bind(pid) -> watchpoints, and capture(hit_indices) -> a JSON
snapshot with an events list. Each watchpoint is (host_address, byte_width).
Bindings verify executable identity, decode service boundaries, and include
selected animation, game clock and mixer generation in each event. No sampled
animation group or PCM amplitude is converted into a sound-service event here.
"""
import argparse
import ctypes
import fcntl
import importlib.util
import json
import math
import os
import platform
from pathlib import Path
import signal
import time


class WriteWatches:
    """Linux x86-64 hardware write watches; detach always clears owned watches."""
    def __init__(self, pid):
        if platform.system() != 'Linux' or platform.machine() not in ('x86_64', 'AMD64'):
            raise RuntimeError('original sound watches require Linux x86-64')
        self.pid = pid
        self.attached = False
        self.stopped = False
        self.lib = ctypes.CDLL(None, use_errno=True)
        self.lib.ptrace.restype = ctypes.c_long
        self.lib.ptrace.argtypes = [ctypes.c_uint, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p]

    def trace(self, operation, address=0, value=0):
        ctypes.set_errno(0)
        result = self.lib.ptrace(operation, self.pid, address, value)
        error = ctypes.get_errno()
        if result == -1 and error:
            raise OSError(error, os.strerror(error))
        return result

    def wait(self, deadline):
        # Block SIGCHLD before checking waitpid so a stop cannot arrive between
        # the check and the wait. Timed signal waiting bounds a silent emulator
        # without adding a polling sleep to every trapped audio-service call.
        previous = signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGCHLD})
        try:
            while True:
                pid, status = os.waitpid(self.pid, os.WNOHANG)
                if pid:
                    if not os.WIFSTOPPED(status):
                        self.attached = False
                        raise RuntimeError('observed emulator exited')
                    self.stopped = True
                    return os.WSTOPSIG(status)
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError('sound observer wall deadline expired')
                signal.sigtimedwait({signal.SIGCHLD}, remaining)
        finally:
            signal.pthread_sigmask(signal.SIG_SETMASK, previous)

    def attach(self):
        self.trace(16)
        self.attached = True
        self.wait(time.monotonic() + 5)

    def install(self, watches):
        if not self.stopped or not 1 <= len(watches) <= 4:
            raise ValueError('one to four watches require a stopped emulator')
        control = 0
        for index, (address, width) in enumerate(watches):
            if width not in (1, 2, 4, 8) or address <= 0 or address % width:
                raise ValueError('invalid or unaligned hardware watch')
            self.trace(6, 848 + 8 * index, address)
            length = {1: 0, 2: 1, 4: 3, 8: 2}[width]
            control |= 1 << (2 * index) | (1 | length << 2) << (16 + 4 * index)
        self.trace(6, 896, 0)
        self.trace(6, 904, control)

    def next(self, deadline):
        self.trace(7)
        self.stopped = False
        if self.wait(deadline) != signal.SIGTRAP:
            raise RuntimeError('unexpected signal at sound-service boundary')
        hits = self.trace(3, 896) & 15
        self.trace(6, 896, 0)
        if not hits:
            raise RuntimeError('sound-service trap has no owned watch event')
        return [index for index in range(4) if hits & 1 << index]

    def close(self):
        if self.attached:
            if not self.stopped:
                os.kill(self.pid, signal.SIGSTOP)
                self.wait(time.monotonic() + 5)
            self.trace(6, 904, 0)
            self.trace(17)
            self.attached = False


def validate_event(event):
    """Require atomic clock/animation identity and slot generation at each event."""
    if not isinstance(event, dict) or event.get('kind') not in ('play', 'remember', 'stop', 'complete', 'forget'):
        raise ValueError('unknown sound-service event')
    state = event.get('snapshot', {})
    generations = state.get('mixer_generation')
    if (type(state.get('frame')) is not int or state['frame'] < 0
            or type(state.get('game_seconds')) not in (int, float)
            or not math.isfinite(state['game_seconds']) or state['game_seconds'] < 0
            or not state.get('owner') or not state.get('animation')
            or type(state.get('group')) is not int
            or not isinstance(generations, list) or len(generations) != 16
            or any(type(value) is not int or value < 0 for value in generations)):
        raise ValueError('sound event lacks atomic animation/clock/mixer identity')
    slot = event.get('slot')
    if event['kind'] == 'forget':
        if slot != -1:
            raise ValueError('forgotten animation sample must have absent slot')
    elif (type(slot) is not int or not 0 <= slot < 16
            or type(event.get('generation')) is not int or event['generation'] <= 0
            or generations[slot] != event['generation']):
        raise ValueError('sound event lacks observed sample generation')


def record(binding, watches, publish, stopped, deadline, event_limit):
    """Publish complete stopped snapshots; event loss fails the entire stream."""
    count = 0
    while not stopped():
        hits = watches.next(deadline)
        snapshot = binding.capture(hits)
        if (not isinstance(snapshot, dict) or type(snapshot.get('events')) is not list
                or snapshot.get('overflow') is not False):
            raise ValueError('sound observer missing events or explicit no-overflow acknowledgement')
        if count + len(snapshot['events']) > event_limit:
            raise ValueError('sound observer event buffer overflow')
        for event in snapshot['events']:
            validate_event(event)
            count += 1
            if type(event.get('sequence')) is not int or event['sequence'] != count:
                raise ValueError('sound observer missing or repeated event sequence')
        publish(snapshot)
    return count


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('bindings', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--deadline', type=float, default=120)
    parser.add_argument('--event-limit', type=int, default=4096)
    args = parser.parse_args()
    if not 0 < args.deadline < float('inf') or args.event_limit <= 0:
        parser.error('deadline and event limit must be positive and finite')
    spec = importlib.util.spec_from_file_location('original_sound_bindings', args.bindings)
    binding = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(binding)
    args.output.mkdir(parents=True, exist_ok=True)
    def status(value):
        path = args.output / 'status.tmp'
        path.write_text(json.dumps(value) + '\n')
        path.replace(args.output / 'status.json')
    with (args.output / 'observer.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if (args.output / 'events.jsonl').exists():
            raise ValueError('preserve previous observer stream; use a fresh output directory')
        observer = WriteWatches(binding.pid())
        try:
            try:
                observer.attach()
                observer.install(binding.bind(observer.pid))
                status(dict(state='armed', overflow=False))
                with (args.output / 'events.jsonl').open('x') as stream:
                    def publish(snapshot):
                        stream.write(json.dumps(snapshot) + '\n')
                        stream.flush()
                        if snapshot['events']:
                            os.fsync(stream.fileno())
                    count = record(binding, observer, publish,
                        lambda: (args.output / 'stop').exists(),
                        time.monotonic() + args.deadline, args.event_limit)
            finally:
                observer.close()
            status(dict(state='complete', overflow=False, events=count))
        except BaseException as error:
            status(dict(state='rejected', error=str(error)))
            raise


if __name__ == '__main__':
    main()
