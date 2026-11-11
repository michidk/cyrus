#!/usr/bin/env python3
"""Replay an accepted original rope input schedule through the native control port.

Uses ordinary world-update counts from the original JSONL, not host sleeps.
This compares per-update input; it does not establish equal elapsed game time.
Audio and attachment evidence remain explicit in the resulting record.
"""
import argparse
import importlib.util
import hashlib
import fcntl
import os
import json
from pathlib import Path
import time

spec = importlib.util.spec_from_file_location('native_trace', Path(__file__).with_name('native-trace.py'))
trace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trace)


def accepted_schedule(path):
    rows = [json.loads(line) for line in path.read_text().splitlines()]
    phases = {}
    for row in rows:
        if row['phase'] in ('placed', 'walk-in', 'attack', 'continuation'):
            player = row['player']
            if not player['drawn'] or player['falling'] or player['living'] or player['health'] <= 0:
                raise ValueError('original contact baseline was rejected')
            phases.setdefault(row['phase'], []).append(row)
    if set(phases) != {'placed', 'walk-in', 'attack', 'continuation'}:
        raise ValueError('original contact run is incomplete')
    schedule = {}
    for phase in ('walk-in', 'attack', 'continuation'):
        frames = [row['frame'] for row in phases[phase]]
        if any(b < a for a, b in zip(frames, frames[1:])) or frames[-1] <= frames[0]:
            raise ValueError(f'original {phase} clock did not advance')
        schedule[phase] = frames[-1] - frames[0]
    return phases['placed'][-1]['player'], schedule


def save_progress(path, value):
    temporary = path.with_suffix(path.suffix + '.tmp')
    with temporary.open('w') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)
    descriptor = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def prepare_progress(path, original, output, resume):
    identity = hashlib.sha256(original.read_bytes()).hexdigest()
    if path.exists():
        if not resume:
            raise ValueError('progress exists; use --resume or a new output path')
        value = json.loads(path.read_text())
        if value.get('version') != 1 or value.get('status') not in {'waiting', 'ready', 'running', 'completed'}:
            raise ValueError('unsupported or corrupt progress record')
        if value['original_sha256'] != identity or value['output'] != str(output.resolve()):
            raise ValueError('resume source or output changed')
        if value['status'] == 'running':
            raise ValueError('input attempt was interrupted; preserve it and use a new output path')
        return value
    if resume:
        raise ValueError('no saved progress to resume')
    if output.exists():
        raise ValueError('output already exists; preserve it and use a new output path')
    value = dict(version=1, original_sha256=identity, output=str(output.resolve()),
                 status='waiting', polls=0)
    save_progress(path, value)
    return value


def wait_ready(driver, seconds, path, progress):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        try:
            state = driver.call('state', _timeout=max(.001, deadline - time.monotonic()))
        except (OSError, ValueError) as error:
            progress['last_observation'] = str(error)
        else:
            scripts = state.get('scripts') or {}
            progress['last_observation'] = dict(state=state.get('state'),
                scene=state.get('scene'), scene_ready=state.get('scene_ready'),
                scripts=scripts.get('state'), ordinary_updates=scripts.get('ordinary_updates'),
                error=scripts.get('error'))
            if scripts.get('error'):
                save_progress(path, progress)
                raise RuntimeError(scripts['error'])
            if (state.get('state') == 'InGame' and state.get('scene') == 'CATACOMB'
                    and state.get('scene_ready') and scripts.get('state') == 'Loaded'
                    and scripts.get('ordinary_updates', 0) > 0):
                progress['status'] = 'ready'
                save_progress(path, progress)
                return
        progress['polls'] += 1
        save_progress(path, progress)
        time.sleep(.2)
    raise RuntimeError('native readiness timed out; saved progress can be resumed')


class Driver:
    def __init__(self, addr, timeout, output):
        self.addr, self.timeout, self.output = addr, timeout, output
        self.index = None
        self.held = set()

    def call(self, kind, _timeout=None, **args):
        timeout = self.timeout if _timeout is None else min(self.timeout, _timeout)
        return trace.call(self.addr, dict(type=kind, **args), timeout=timeout)

    def sample(self, phase):
        state = self.call('state')
        scripts = state.get('scripts') or {}
        if scripts.get('error'):
            raise RuntimeError(scripts['error'])
        if state.get('scene') != 'CATACOMB' or not state.get('scene_ready') or scripts.get('state') != 'Loaded':
            raise RuntimeError('native world is not ready')
        if self.index is None:
            self.index = trace.player_index(self.addr)
        actor, = self.call('scripts', offset=self.index, limit=1)['actors']
        control = actor['control']
        row = dict(phase=phase, frame=scripts['ordinary_updates'],
                   game_seconds=state['clock']['game_seconds'],
                   player=dict(drawn=control['weapon_drawn'], falling=control['falling_state'],
                               living=control['living_state'], rope_attached=control['rope_attached'],
                               health=actor['attributes'][3],
                               position=[v / 256 for v in actor['spatial']['effective']['position']],
                               group=actor['animation']['current_group']))
        self.output.write(json.dumps(row) + '\n')
        self.output.flush()
        return row

    def progress(self, frames, phase, validate=False):
        start = self.sample(phase)['frame']
        deadline = time.monotonic() + self.timeout
        previous = start
        while time.monotonic() < deadline:
            row = self.sample(phase)
            if row['frame'] < previous:
                raise RuntimeError('native world clock restarted')
            previous = row['frame']
            if validate:
                player = row['player']
                if not player['drawn'] or player['falling'] or player['living'] or player['health'] <= 0:
                    raise RuntimeError('native contact baseline was rejected')
            if row['frame'] - start >= frames:
                return row
            time.sleep(.02)
        raise RuntimeError('native world clock stalled')

    def key(self, key, pressed):
        self.call('input', key=key, action='press' if pressed else 'release')
        if pressed:
            self.held.add(key)
        else:
            self.held.discard(key)

    def run(self, original, schedule):
        try:
            self.progress(1, 'ready')
            self.call('cheat', name='magiccarpet', value=1)
            self.call('console', line='item add,1')
            if not self.sample('inventory')['player']['drawn']:
                self.key('KeyS', True)
                self.progress(2, 'draw')
                self.key('KeyS', False)
            self.progress(12, 'settle')
            x, y, z = original['position']
            self.call('place_player', x=x, y=y, z=z, heading=original['yaw'])
            self.progress(2, 'placed', validate=True)
            for phase, key in [('walk-in', 'ArrowUp'), ('attack', 'ControlLeft'), ('continuation', None)]:
                if key:
                    self.key(key, True)
                try:
                    self.progress(schedule[phase], phase, validate=True)
                finally:
                    if key:
                        self.key(key, False)
        finally:
            for key in tuple(self.held):
                self.key(key, False)


def execute(args):
    original, schedule = accepted_schedule(args.original)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.with_suffix(args.output.suffix + '.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        execute_locked(args, original, schedule)


def execute_locked(args, original, schedule):
    progress_path = args.output.with_suffix(args.output.suffix + '.progress.json')
    progress = prepare_progress(progress_path, args.original, args.output, args.resume)
    if progress['status'] == 'completed':
        if not args.output.is_file() or hashlib.sha256(args.output.read_bytes()).hexdigest() != progress['output_sha256']:
            raise ValueError('completed output is missing or changed')
        return
    driver = Driver(args.addr, args.timeout, None)
    wait_ready(driver, args.ready_timeout, progress_path, progress)
    progress['status'] = 'running'
    save_progress(progress_path, progress)
    with args.output.open('x') as output:
        driver.output = output
        driver.run(original, schedule)
        output.flush()
        os.fsync(output.fileno())
    progress.update(status='completed', output_sha256=hashlib.sha256(args.output.read_bytes()).hexdigest())
    save_progress(progress_path, progress)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--addr', required=True)
    parser.add_argument('--original', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--timeout', type=float, default=35)
    parser.add_argument('--resume', action='store_true',
                        help='resume readiness or acknowledge completed output; never replay interrupted input')
    parser.add_argument('--ready-timeout', type=float, default=180)
    args = parser.parse_args()
    if args.timeout <= 0 or args.ready_timeout <= 0:
        parser.error('timeout must be positive')
    execute(args)


if __name__ == '__main__':
    main()
