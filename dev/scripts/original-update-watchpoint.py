#!/usr/bin/env python3
"""Linux x86-64 stopped-boundary backend for original-update-recorder.py.

ORIGINAL_UPDATE_CONFIG names an ignored JSON binding: reader (Python state
reader), boundary_guest_offset (verified update-start write), counter, clock,
step (reader keys), and player_fields (output name to selected actor key).
The reader supplies find_pid(), find_guest(pid), and main() producing JSON.
No executable offsets or signatures are distributed with this backend.
"""
from contextlib import contextmanager, redirect_stdout
import ctypes
import importlib.util
import io
import json
import os
import platform
import signal
import struct
import time


def clock_snapshot(state, config):
    """Normalize a verified publication stop to completed-movement clock units.

    The published accumulator includes the pending movement step. Counter-entry
    stops precede measurement and cannot supply that forthcoming step. Timer
    candidates must agree with the engine's observed measurement sample.
    """
    phase = config.get('clock_phase', 'completed_update')
    if phase not in ('completed_update', 'published_pending_update'):
        raise ValueError(f'unsupported original clock phase: {phase}')
    total, step = field(state, config['clock']), field(state, config['step'])
    timing = {} if 'bios' not in config else dict(bios_ticks=field(state, config['bios']))
    if phase == 'completed_update':
        return dict(clock_scale=total, next_step=step, **timing)
    pending = field(state, config['pending_step'])
    warmup = field(state, config['warmup'])
    sample = field(state, config['measurement_bios'])
    candidate = timing.get('bios_ticks')
    if (type(sample) is not int or not 0 <= sample <= 2**32 - 1
            or type(candidate) is not int or candidate != sample):
        raise ValueError('BIOS candidate differs from observed engine measurement sample')
    if (type(total) is not int or type(step) is not int or type(pending) is not int
            or pending <= 0 or step != pending or total < pending or warmup != 0):
        raise ValueError('publication requires a settled matching pending accumulator/movement step')
    return dict(clock_scale=total - pending, next_step=step, **timing,
                clock_publication=dict(accumulator=total, pending_step=pending,
                                       movement_step=step, bios_sample=sample))


class Watchpoint:
    def __init__(self, config, reader):
        self.config = config
        self.reader = reader
        self.pid = reader.find_pid()
        self.memory, self.base = reader.find_guest(self.pid)
        self.libc = ctypes.CDLL(None, use_errno=True)
        self.libc.ptrace.restype = ctypes.c_long
        self.libc.ptrace.argtypes = [ctypes.c_uint, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p]
        self.attached = False
        self.stopped = False
        self.saved_registers = None
        self.active_boundary = None
        self.observations = {}

    def ptrace(self, operation, offset=0, value=0):
        ctypes.set_errno(0)
        result = self.libc.ptrace(operation, self.pid, offset, value)
        error = ctypes.get_errno()
        if result == -1 and error:
            name = {16: 'PTRACE_ATTACH', 17: 'PTRACE_DETACH'}.get(operation, f'ptrace request {operation}')
            raise OSError(error, f'original observer {name} for pid {self.pid}: {os.strerror(error)}; '
                          'run original_observer_preflight.py in the same runtime')
        return result

    def wait(self, timeout):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                if not os.WIFSTOPPED(status):
                    raise RuntimeError('original emulator exited during recording')
                self.stopped = True
                return os.WSTOPSIG(status)
            time.sleep(.001)
        raise TimeoutError('original ordinary-update boundary stalled')

    def attach(self):
        if platform.machine() != 'x86_64':
            raise RuntimeError('hardware watchpoint backend requires Linux x86-64')
        # Linux struct user debug-register slots; these are host ABI, not game bindings.
        self.ptrace(16)
        self.attached = True
        self.wait(5)
        self.saved_registers = [self.ptrace(3, 848 + index * 8) for index in (0, 1, 2, 3, 6, 7)]
        if self.saved_registers[-1] & 255:
            raise RuntimeError('emulator already has active hardware breakpoints')
        self.ptrace(6, 848, self.base + self.config['boundary_guest_offset'])
        self.ptrace(6, 848 + 7 * 8, self._debug_control())
        self.active_boundary = self.config['boundary_guest_offset']

    def pause(self, timeout=5):
        """Stop this attached emulator so failed input can be released safely."""
        if not self.attached:
            raise RuntimeError('cannot pause an unattached emulator')
        if not self.stopped:
            os.kill(self.pid, signal.SIGSTOP)
            self.wait(timeout)

    def select_boundary(self, guest_offset):
        """Rebind the stopped write watchpoint without advancing the emulator.

        The caller supplies an evidenced local loader or ordinary-frame binding.
        Selecting a binding is not evidence that restoration has completed; the
        resulting stopped state must still pass the checkpoint contract.
        """
        if not self.stopped:
            raise RuntimeError('cannot rebind a running emulator')
        if type(guest_offset) is not int or guest_offset < 0 or guest_offset % 4:
            raise ValueError('boundary requires an aligned nonnegative guest offset')
        self.ptrace(6, 848 + 7 * 8, 0)
        self.ptrace(6, 848, self.base + guest_offset)
        self.ptrace(6, 848 + 6 * 8, 0)
        self.ptrace(6, 848 + 7 * 8, self._debug_control())
        self.active_boundary = guest_offset

    def _debug_control(self):
        control = 1 | (1 << 16) | (3 << 18)
        for slot, (_, size, _) in self.observations.items():
            length = {1: 0, 2: 1, 4: 3, 8: 2}[size]
            control |= (1 << (2 * slot)) | ((1 | (length << 2)) << (16 + slot * 4))
        return control

    def observe_write(self, slot, guest_offset, size, callback):
        """Observe a bound mid-update write without treating it as a frame boundary.

        Slots 1..3 are separate from the ordinary-frame slot. Callbacks run while
        stopped and may rebind another observer. Executable/reader bindings and
        identity validation belong to the local caller. This never changes guest
        code or data. A callback error propagates with the emulator still stopped.
        """
        if not self.stopped:
            raise RuntimeError('cannot bind an observation in a running emulator')
        if (type(slot) is not int or slot not in (1, 2, 3)
                or type(guest_offset) is not int or guest_offset < 0
                or type(size) is not int or size not in (1, 2, 4, 8)
                or (self.base + guest_offset) % size or not callable(callback)):
            raise ValueError('write observation requires a spare slot and aligned supported size')
        self.ptrace(6, 848 + 7 * 8, 0)
        self.ptrace(6, 848 + slot * 8, self.base + guest_offset)
        self.observations[slot] = (guest_offset, size, callback)
        self.ptrace(6, 848 + 7 * 8, self._debug_control())

    def stop_observing_write(self, slot):
        """Disarm a completed observation before its transient storage is reused."""
        if not self.stopped:
            raise RuntimeError('cannot disarm an observation in a running emulator')
        if type(slot) is not int or slot not in (1, 2, 3):
            raise ValueError('write observation requires a spare slot')
        self.observations.pop(slot, None)
        self.ptrace(6, 848 + 7 * 8, self._debug_control())

    def next_boundary(self, timeout, max_observations=64):
        """Resume to the next ordinary boundary, retaining bounded write observations."""
        if type(max_observations) is not int or max_observations < 0:
            raise ValueError('observation budget must be a nonnegative integer')
        deadline, observed = time.monotonic() + timeout, 0
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError('original ordinary-update boundary stalled')
            self.ptrace(7)
            self.stopped = False
            if self.wait(remaining) != signal.SIGTRAP:
                raise RuntimeError('unexpected signal while waiting for ordinary boundary')
            stops = self.ptrace(3, 848 + 6 * 8) & 15
            known = 1 | sum(1 << slot for slot in self.observations)
            if not stops or stops & ~known:
                raise RuntimeError('unidentified original debug stop')
            self.ptrace(6, 848 + 6 * 8, 0)
            for slot, (_, _, callback) in list(self.observations.items()):
                if stops & (1 << slot):
                    observed += 1
                    if observed > max_observations:
                        raise RuntimeError('original write observation budget exhausted')
                    callback()
            if stops & 1:
                return

    def raw_snapshot(self):
        """Batch every requested observable while the emulated game is stopped."""
        if not self.stopped:
            raise RuntimeError('cannot snapshot a running emulator')
        output = io.StringIO()
        with redirect_stdout(output):
            self.reader.main()
        return json.loads(output.getvalue())

    def latch_key(self, key, action):
        """Latch an ordinary key slot without running a frame; verify through the reader.

        The local binding supplies the documented 16-bit keyboard-state slot and
        its reader path. Special interrupt-handled keys need a different adapter.
        A make writes one and a break writes zero (preservation keys-ini.md).
        """
        if not self.stopped:
            raise RuntimeError('cannot latch input in a running emulator')
        if action not in ('press', 'release') or key not in self.config.get('keys', {}):
            raise ValueError(f'unsupported ordinary key {key}/{action}')
        binding = self.config['keys'][key]
        before = self.raw_snapshot()
        counter = field(before, self.config['counter'])
        with open(f'/proc/{self.pid}/mem', 'r+b', buffering=0) as memory:
            memory.seek(self.base + binding['guest_offset'])
            if memory.write(struct.pack('<H', int(action == 'press'))) != 2:
                raise OSError('short original key-state write')
        after = self.raw_snapshot()
        if field(after, self.config['counter']) != counter:
            raise RuntimeError('original frame advanced while latching input')
        if field(after, binding['reader']) != int(action == 'press'):
            raise RuntimeError(f'original reader did not acknowledge {key}/{action}')
        return after

    def snapshot(self):
        if self.active_boundary != self.config['boundary_guest_offset']:
            raise RuntimeError('ordinary snapshot requires its ordinary boundary binding')
        state = self.raw_snapshot()
        selected = [actor for actor in state['actors'] if actor['selected']]
        if len(selected) != 1 or state.get('console_mode') != 'closed':
            raise RuntimeError('ordinary world/player boundary unavailable')
        return dict(execution='stopped', boundary='ordinary_update_start',
                    **clock_snapshot(state, self.config), update=field(state, self.config['counter']),
                    player={name: selected[0][key] for name, key in self.config['player_fields'].items()})

    def close(self):
        if self.attached:
            self.pause()
            if self.saved_registers is not None:
                self.ptrace(6, 848 + 7 * 8, 0)
                for index, value in zip((0, 1, 2, 3, 6, 7), self.saved_registers):
                    self.ptrace(6, 848 + index * 8, value)
            self.ptrace(17)
            self.attached = False
        self.memory.close()


def field(state, path):
    """Read a bound dict/list path; unavailable fields remain errors, never defaults."""
    for key in path.split('.'):
        state = state[int(key)] if isinstance(state, list) else state[key]
    return state


@contextmanager
def open_session():
    with open(os.environ['ORIGINAL_UPDATE_CONFIG']) as source:
        config = json.load(source)
    spec = importlib.util.spec_from_file_location('original_state_reader', config['reader'])
    reader = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(reader)
    session = Watchpoint(config, reader)
    try:
        session.attach()
        yield session
    finally:
        session.close()
