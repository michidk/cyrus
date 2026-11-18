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
import time


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

    def ptrace(self, operation, offset=0, value=0):
        ctypes.set_errno(0)
        result = self.libc.ptrace(operation, self.pid, offset, value)
        error = ctypes.get_errno()
        if result == -1 and error:
            raise OSError(error, os.strerror(error))
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
        self.saved_registers = [self.ptrace(3, 848 + index * 8) for index in (0, 6, 7)]
        if self.saved_registers[2] & 255:
            raise RuntimeError('emulator already has active hardware breakpoints')
        self.ptrace(6, 848, self.base + self.config['boundary_guest_offset'])
        self.ptrace(6, 848 + 7 * 8, 1 | (1 << 16) | (3 << 18))

    def next_boundary(self, timeout):
        self.ptrace(7)
        self.stopped = False
        if self.wait(timeout) != signal.SIGTRAP:
            raise RuntimeError('unexpected signal while waiting for ordinary boundary')
        if not self.ptrace(3, 848 + 6 * 8) & 1:
            raise RuntimeError('unidentified original debug stop')
        self.ptrace(6, 848 + 6 * 8, 0)

    def snapshot(self):
        if not self.stopped:
            raise RuntimeError('cannot snapshot a running emulator')
        output = io.StringIO()
        with redirect_stdout(output):
            self.reader.main()
        state = json.loads(output.getvalue())
        selected = [actor for actor in state['actors'] if actor['selected']]
        if len(selected) != 1 or state.get('console_mode') != 'closed':
            raise RuntimeError('ordinary world/player boundary unavailable')
        def field(path):
            value = state
            for key in path.split('.'):
                value = value[key]
            return value
        return dict(execution='stopped', boundary='ordinary_update_start',
                    update=field(self.config['counter']), clock_scale=field(self.config['clock']),
                    next_step=field(self.config['step']),
                    player={name: selected[0][key] for name, key in self.config['player_fields'].items()})

    def close(self):
        if self.attached:
            if not self.stopped:
                os.kill(self.pid, signal.SIGSTOP)
                self.wait(5)
            if self.saved_registers is not None:
                self.ptrace(6, 848 + 7 * 8, 0)
                for index, value in zip((0, 6, 7), self.saved_registers):
                    self.ptrace(6, 848 + index * 8, value)
            self.ptrace(17)
            self.attached = False
        self.memory.close()


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
