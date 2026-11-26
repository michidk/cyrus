#!/usr/bin/env python3
"""Diagnose owned-child observer attach in the current runtime, without game data.

This checks one ready, same-identity, dumpable direct child and cleans it up.
Success establishes attach/wait/detach capability only, not executable bindings,
hardware breakpoint ownership, game readiness or combat equivalence. Run inside
the actual observer runtime. It neither grants permissions nor changes policy.
"""
import argparse
import ctypes
import json
import os
from pathlib import Path
import select
import signal
import subprocess
import sys
import time

from native_session_capture import atomic, file_digest

CHILD = '''import ctypes,json,os,sys
libc=ctypes.CDLL(None)
print(json.dumps(dict(pid=os.getpid(),ppid=os.getppid(),uid=os.getuid(),
 euid=os.geteuid(),dumpable=libc.prctl(3,0,0,0,0))),flush=True)
sys.stdin.read()
'''


def policy():
    """Record readable policy facts without attributing a denial to one of them."""
    result = {}
    for key, name in [('status', '/proc/self/status'), ('yama_ptrace_scope', '/proc/sys/kernel/yama/ptrace_scope'),
                      ('lsm_context', '/proc/self/attr/current')]:
        try:
            text = Path(name).read_text().strip()
            result[key] = ([line for line in text.splitlines() if line.startswith(
                ('CapEff:', 'CapBnd:', 'NoNewPrivs:', 'Seccomp:', 'Seccomp_filters:'))]
                if key == 'status' else text)
        except OSError as error:
            result[key] = dict(errno=error.errno, error=str(error))
    return result


def probe(child, ready, trace, wait, *, parent_pid, uid, euid):
    report = dict(supported=False, scope='owned-child-attach-wait-detach',
                  parent=dict(pid=parent_pid, uid=uid, euid=euid), child=ready, stage='readiness')
    expected = dict(pid=child.pid, ppid=parent_pid, uid=uid, euid=euid, dumpable=1)
    if ready != expected:
        return dict(report, error='child readiness/identity/dumpability does not match owned launch')
    attached = False
    try:
        report['stage'] = 'PTRACE_ATTACH'
        trace(16, child.pid)
        attached = True
        report['stage'] = 'wait_stopped'
        wait(child.pid)
        report['stage'] = 'PTRACE_DETACH'
        trace(17, child.pid)
        attached = False
        return dict(report, supported=True, stage='complete')
    except (OSError, RuntimeError, TimeoutError) as error:
        return dict(report, errno=getattr(error, 'errno', None), error=str(error))
    finally:
        if attached:
            try:
                trace(17, child.pid)
            except OSError:
                # The caller still owns termination and reaping, even if detach fails.
                pass


def wait_stopped(pid, timeout=3):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        found, status = os.waitpid(pid, os.WNOHANG)
        if found:
            if not os.WIFSTOPPED(status):
                raise RuntimeError('owned child exited before its attach stop')
            if os.WSTOPSIG(status) != signal.SIGSTOP:
                raise RuntimeError('owned child stopped with an unexpected signal')
            return
        time.sleep(.005)
    raise TimeoutError('owned child did not acknowledge attach stop')


def run(timeout=3):
    report = dict(supported=False, stage='launch', policy=policy(),
                  tool_sha256=file_digest(Path(__file__)))
    if sys.platform != 'linux':
        return dict(report, error='observer preflight requires Linux')
    libc = ctypes.CDLL(None, use_errno=True)
    libc.ptrace.restype = ctypes.c_long
    libc.ptrace.argtypes = [ctypes.c_uint, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p]
    def trace(operation, pid):
        ctypes.set_errno(0)
        value = libc.ptrace(operation, pid, None, None)
        code = ctypes.get_errno()
        if value == -1 and code:
            raise OSError(code, os.strerror(code))
    try:
        child = subprocess.Popen([sys.executable, '-I', '-u', '-c', CHILD],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    except OSError as error:
        return dict(report, error=str(error), errno=error.errno)
    try:
        report['stage'] = 'readiness'
        if not select.select([child.stdout], [], [], timeout)[0]:
            raise TimeoutError('owned child readiness handshake timed out')
        ready = json.loads(child.stdout.readline())
        report.update(probe(child, ready, trace, lambda pid: wait_stopped(pid, timeout),
                            parent_pid=os.getpid(), uid=os.getuid(), euid=os.geteuid()))
    except (OSError, ValueError, RuntimeError, TimeoutError) as error:
        report.update(error=str(error), errno=getattr(error, 'errno', None))
    finally:
        # A stopped/traced child must not outlive the diagnostic, even on errors.
        if child.poll() is None:
            child.kill()
        child.communicate(timeout=timeout)
        report['child_reaped'] = child.returncode is not None
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, help='atomically retain the diagnostic receipt')
    args = parser.parse_args()
    report = run()
    data = (json.dumps(report, indent=2, sort_keys=True) + '\n').encode()
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        atomic(args.out, data)
    print(data.decode(), end='')
    return 0 if report['supported'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
