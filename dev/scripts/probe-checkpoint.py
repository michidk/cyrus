#!/usr/bin/env python3
"""Durable readiness and ordinary-boundary receipts for bounded runtime probes.

Drivers write this journal on a host-mounted evidence directory before sending
input and at each acknowledged ordinary boundary. Inspect after interruption;
restart replays an incomplete attempt from its fixture, never from a stale pose.
Accepted receipts survive container loss. A receipt records acquisition, not
original/native gameplay parity; counterpart clocks still require exact matching.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import time
import uuid


def save(path, record):
    """Publish one complete JSON receipt, including directory metadata, before returning."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix('.tmp')
    with temporary.open('w') as stream:
        json.dump(record, stream, indent=2, allow_nan=False)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)
    descriptor = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def container_alive(container):
    result = subprocess.run(['docker', 'inspect', '--format', '{{.State.Running}}', container],
                            capture_output=True, text=True, timeout=10)
    if result.returncode:
        # A stopped daemon is an observation failure, not proof of a lost container.
        subprocess.run(['docker', 'info'], check=True, capture_output=True, timeout=10)
    return result.returncode == 0 and result.stdout.strip() == 'true'


def register_job(path):
    """Retain PID plus process birth time so PID reuse cannot masquerade as a live probe."""
    ticks = Path('/proc/self/stat').read_text().rpartition(') ')[2].split()[19]
    save(Path(path), dict(pid=os.getpid(), start_ticks=ticks))


def job_alive(container, pidfile):
    code = ("import json,sys; from pathlib import Path; "
            "job=json.loads(Path(sys.argv[1]).read_text()); "
            "ticks=Path('/proc/%s/stat'%job['pid']).read_text().rpartition(') ')[2].split()[19]; "
            "sys.exit(0 if ticks==job['start_ticks'] else 1)")
    return subprocess.run(['docker', 'exec', container, 'python3', '-c', code, pidfile],
                          capture_output=True, timeout=10).returncode == 0


def inspect_run(path, pidfile=None):
    record = json.loads(path.read_text())
    if record['status'] not in ('accepted', 'rejected', 'lost'):
        alive = container_alive(record['container'])
        if not alive or (pidfile is not None and not job_alive(record['container'], pidfile)):
            record.update(status='lost', reason='container absent/stopped' if not alive else 'probe job absent')
            save(path, record)
    return record


class Run:
    """One container lifetime and a contiguous measured stage; use a per-run lock outside it."""
    def __init__(self, path, record):
        self.path, self.record = path, record

    @classmethod
    def start(cls, path, container, fixture, restart=False):
        path = Path(path)
        if type(fixture.get('target_scale')) is not int or fixture['target_scale'] <= 0:
            raise ValueError('probe fixture needs a positive integer target_scale')
        if path.exists():
            old = json.loads(path.read_text())
            if old['fixture'] != fixture:
                raise ValueError('probe fixture changed; use a new evidence path')
            if not restart:
                raise ValueError('probe exists; inspect and attach before restarting')
            if old['status'] == 'accepted':
                raise ValueError('accepted probe must be reused, not restarted')
            if old['status'] != 'lost' and container_alive(old['container']):
                raise ValueError('live probe must be attached, not duplicated')
            save(path.with_name(f'{path.stem}.{old["attempt"]}.json'), old)
        record = dict(version=1, attempt=uuid.uuid4().hex, container=container,
                      fixture=fixture, status='waiting', polls=0, boundaries=[])
        save(path, record)
        return cls(path, record)

    def wait_ready(self, observe, timeout, interval=1):
        """Persist loading/connection failures until the selected actor has an update boundary."""
        deadline = time.monotonic() + timeout
        while True:
            try:
                row = observe()
            except OSError as error:
                row = dict(readiness='unavailable', selected_count=0, frame=0, error=str(error))
            self.readiness(row)
            if (row.get('readiness') == 'selected' and row.get('selected_count') == 1
                    and row.get('frame', 0) > 2):
                return row
            if time.monotonic() >= deadline:
                raise TimeoutError(f'probe actor readiness expired: {row}')
            time.sleep(interval)

    def progress(self, observation):
        """Retain non-atomic loading/clock diagnostics separately from acceptance boundaries."""
        if self.record['status'] not in ('waiting', 'running'):
            raise ValueError('probe is no longer active')
        record = dict(self.record, progress=observation)
        save(self.path, record)
        self.record = record

    def readiness(self, observation):
        if self.record['status'] != 'waiting':
            raise ValueError('readiness belongs before the input attempt')
        record = dict(self.record, readiness=observation, polls=self.record['polls'] + 1)
        save(self.path, record)
        self.record = record

    def boundary(self, phase, generation, update, consumed_scale, next_scale, player):
        ready = self.record.get('readiness', {})
        if (ready.get('readiness') != 'selected' or ready.get('selected_count') != 1
                or ready.get('frame', 0) <= 2):
            raise ValueError('probe needs a verified selected actor before input')
        if self.record['status'] not in ('waiting', 'running'):
            raise ValueError('probe is no longer running')
        if (any(type(value) is not int for value in (generation, update, consumed_scale, next_scale))
                or min(generation, update, consumed_scale) < 0 or next_scale <= 0):
            raise ValueError('invalid ordinary-boundary clock')
        row = dict(phase=phase, generation=generation, update=update,
                   consumed_scale=consumed_scale, next_scale=next_scale, player=player)
        previous = self.record.get('last')
        if previous:
            if (phase != previous['phase'] or generation != previous['generation']
                    or update != previous['update'] + 1
                    or consumed_scale != previous['consumed_scale'] + previous['next_scale']):
                raise ValueError('ordinary boundary gap, lifetime or consumed-scale mismatch')
        elif consumed_scale != 0:
            raise ValueError('first boundary must establish the zero-scale origin')
        record = dict(self.record, status='running', last=row,
                      boundaries=self.record['boundaries'] + [row])
        save(self.path, record)
        self.record = record

    def reject(self, reason):
        """Keep an acquisition failure distinct from a subsequently stopped runtime."""
        if self.record['status'] not in ('waiting', 'running'):
            raise ValueError('probe is no longer active')
        record = dict(self.record, status='rejected', reason=reason)
        save(self.path, record)
        self.record = record

    def accept(self, phase):
        last = self.record.get('last', {})
        if (self.record['status'] != 'running' or last.get('phase') != phase
                or last.get('consumed_scale') != self.record['fixture']['target_scale']):
            raise ValueError('probe has not reached its exact target boundary')
        record = dict(self.record, status='accepted')
        save(self.path, record)
        self.record = record


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('path', type=Path)
    parser.add_argument('--pidfile', help='container path to the probe job PID file')
    args = parser.parse_args()
    record = inspect_run(args.path, args.pidfile)
    print(json.dumps(record, indent=2))
    if record['status'] == 'lost':
        raise SystemExit(3)


if __name__ == '__main__':
    main()
