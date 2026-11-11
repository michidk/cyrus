#!/usr/bin/env python3
"""Replay an accepted original rope input schedule through the native control port.

Requires acknowledged input boundaries and cumulative game-clock intervals.
Unmatched or uncertain timing is rejected before comparing displacement/contact.
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
import signal
import subprocess
import wave
from contextlib import nullcontext

spec = importlib.util.spec_from_file_location('native_trace', Path(__file__).with_name('native-trace.py'))
trace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trace)


def interval(start, end):
    """Conservative elapsed-time bounds, including asynchronous admission uncertainty."""
    import math
    try:
        values = [start['clock'][0], start['clock'][1], end['clock'][0], end['clock'][1]]
    except (KeyError, IndexError, TypeError) as error:
        raise ValueError('missing or invalid game clock') from error
    if not all(isinstance(v, (int, float)) and math.isfinite(v) for v in values):
        raise ValueError('missing or invalid game clock')
    a, b, c, d = values
    if not a <= b < c <= d:
        raise ValueError('game clock did not advance across input boundaries')
    return dict(duration=[c - b, d - a], start=start, end=end)


def compare_interval(original, native, tolerance):
    """Require every possible elapsed-time difference to fit the declared error budget."""
    a, b = original['duration']
    c, d = native['duration']
    if max(abs(a - d), abs(b - c)) > tolerance + 1e-12:
        raise ValueError(f'timing mismatch: original {a, b}, native {c, d}, budget {tolerance}')


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
        edges = [row for row in phases[phase] if row.get('boundary')]
        boundaries = {row['boundary']: row for row in edges}
        if len(edges) != 2 or set(boundaries) != {'start', 'end'}:
            raise ValueError(f'original {phase} lacks acknowledged game-clock boundaries; acquire timing samples')
        if not all(row.get('input_ack') is True for row in boundaries.values()):
            raise ValueError(f'original {phase} input was not acknowledged')
        schedule[phase] = interval(boundaries['start'], boundaries['end'])
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


class MixerCapture:
    """Capture the paced PulseAudio sink monitor, never synthesized/null ALSA output."""
    def __init__(self, path):
        self.path = path

    def __enter__(self):
        if self.path.exists():
            raise ValueError('mixer output exists; preserve it and choose a new path')
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.process = subprocess.Popen(['parec', '--device=cyrus.monitor', '--file-format=wav',
            '--format=s16le', '--rate=48000', '--channels=2', str(self.path)])
        deadline = time.monotonic() + 5
        while not self.path.exists() or self.path.stat().st_size <= 44:
            if self.process.poll() is not None or time.monotonic() >= deadline:
                self.__exit__(RuntimeError, None, None)
                raise RuntimeError('mixer capture did not admit PCM before input')
            time.sleep(.01)
        return self

    def __exit__(self, kind, value, traceback):
        if self.process.poll() is None:
            self.process.send_signal(signal.SIGINT)
        try:
            code = self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
            if kind is None:
                raise RuntimeError('mixer recorder did not stop')
            return False
        try:
            with wave.open(str(self.path)) as audio:
                if code not in (0, -signal.SIGINT) or audio.getnframes() == 0:
                    raise ValueError('empty or failed recording')
                if audio.getparams()[:3] != (2, 2, 48000):
                    raise ValueError('unexpected mixer PCM format')
        except (OSError, EOFError, wave.Error, ValueError) as error:
            if kind is None:
                raise RuntimeError(f'mixer capture failed: {error}') from error
        return False


class Driver:
    def __init__(self, addr, timeout, output, clock_tolerance=0, mixer_path=None):
        self.addr, self.timeout, self.output = addr, timeout, output
        self.mixer_path = mixer_path
        self.clock_tolerance = clock_tolerance
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
        after = self.call('state')
        if (after.get('scene') != 'CATACOMB' or not after.get('scene_ready')
                or (after.get('scripts') or {}).get('state') != 'Loaded'):
            raise RuntimeError('native world changed during sample')
        if after['clock']['game_seconds'] < state['clock']['game_seconds']:
            raise RuntimeError('native game clock restarted during sample')
        control = actor['control']
        row = dict(phase=phase, frame=scripts['ordinary_updates'],
                   game_seconds=state['clock']['game_seconds'],
                   clock_end=after['clock']['game_seconds'],
                   input_state=after.get('player_input', {}),
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
        action = 'press' if pressed else 'release'
        response = self.call('input', key=key, action=action)
        # Track admission even when the response is malformed so cleanup still releases it.
        if pressed:
            self.held.add(key)
        else:
            self.held.discard(key)
        if response != dict(key=key, action=action):
            raise RuntimeError('native input acknowledgement does not match request')

    def timed_segment(self, phase, expected, key):
        # Measured render FPS can differ from the clamped simulation step.
        # Observe one actual ordinary interval to choose an attempt, never to admit it.
        probe = self.call('schedule_input', events=[dict(after_updates=0), dict(after_updates=1)])['boundaries']
        if (len(probe) != 2 or probe[0]['after_updates'] != 0 or probe[1]['after_updates'] != 1
                or probe[1]['update'] != probe[0]['update'] + 1
                or probe[0]['generation'] != probe[1]['generation']
                or any(row.get('key') is not None or row.get('action') is not None for row in probe)):
            raise RuntimeError('native timing probe crossed unavailable ordinary boundaries')
        step_seconds = probe[1]['game_seconds'] - probe[0]['game_seconds']
        if not 0 < step_seconds < float('inf'):
            raise RuntimeError('native ordinary timing probe did not advance')
        target = sum(expected['duration']) / 2
        updates = max(1, round(target / step_seconds))
        events = [dict(after_updates=offset, **(
            dict(key=key, action=action) if key else {}))
            for offset, action in [(0, 'press'), (updates, 'release')]]
        response = self.call('schedule_input', events=events)
        rows = response['boundaries']
        if len(rows) != 2:
            raise RuntimeError('native schedule did not report both boundaries')
        boundaries = []
        for row, event, edge in zip(rows, events, ['start', 'end']):
            if (row['after_updates'] != event['after_updates']
                    or row.get('key') != event.get('key')
                    or row.get('action') != event.get('action')):
                raise RuntimeError('native boundary acknowledgement differs from schedule')
            actor, = row['player']['actors']
            control = actor['control']
            player = dict(drawn=control['weapon_drawn'], falling=control['falling_state'],
                living=control['living_state'], health=actor['attributes'][3],
                rope_attached=control['rope_attached'],
                position=[v / 256 for v in actor['spatial']['effective']['position']])
            boundary = dict(phase=phase, boundary=edge, input_ack=True,
                frame=row['update'], generation=row['generation'],
                clock=[row['game_seconds'], row['game_seconds']], player=player)
            self.output.write(json.dumps(boundary) + '\n')
            self.output.flush()
            if not player['drawn'] or player['falling'] or player['living'] or player['health'] <= 0:
                raise RuntimeError('native contact baseline was rejected')
            boundaries.append(boundary)
        start, end = boundaries
        if (start['generation'] != end['generation']
                or end['frame'] - start['frame'] != updates):
            raise RuntimeError('native schedule crossed unavailable ordinary boundaries')
        observed = interval(start, end)
        try:
            compare_interval(expected, observed, self.clock_tolerance)
        except ValueError as error:
            self.output.write(json.dumps(dict(phase=phase, comparison='rejected',
                duration=observed['duration'], original_duration=expected['duration'],
                clock_tolerance=self.clock_tolerance, error=str(error))) + '\n')
            self.output.flush()
            raise
        result = dict(phase=phase, comparison='matched', duration=observed['duration'],
                      clock_tolerance=self.clock_tolerance,
                      displacement=[b - a for a, b in zip(start['player']['position'], end['player']['position'])],
                      rope_attached=end['player'].get('rope_attached'),
                      original_displacement=[b - a for a, b in zip(expected['start']['player']['position'], expected['end']['player']['position'])],
                      original_rope_attached=expected['end']['player'].get('rope_attached'))
        self.output.write(json.dumps(result) + '\n')
        self.output.flush()

    def run(self, original, schedule):
        try:
            self.progress(1, 'ready')
            self.call('cheat', name='magiccarpet', value=1)
            self.call('console', line='item add,1')
            if not self.sample('inventory')['player']['drawn']:
                response = self.call('input', key='KeyS', action='tap')
                if response != dict(key='KeyS', action='tap'):
                    raise RuntimeError('native sword input acknowledgement does not match request')
                self.progress(2, 'draw')
            self.progress(12, 'settle')
            x, y, z = original['position']
            self.call('place_player', x=x, y=y, z=z, heading=original['yaw'])
            self.progress(2, 'placed', validate=True)
            capture = MixerCapture(self.mixer_path) if self.mixer_path else nullcontext()
            with capture:
                for phase, key in [('walk-in', 'ArrowUp'), ('attack', 'ControlLeft'), ('continuation', None)]:
                    self.timed_segment(phase, schedule[phase], key)
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
    driver = Driver(args.addr, args.timeout, None, args.clock_tolerance, args.record_mixer)
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
    parser.add_argument('--record-mixer', type=Path,
                        help='capture WAV from paced-audio.sh cyrus.monitor in this environment')
    parser.add_argument('--clock-tolerance', type=float, default=0,
                        help='maximum elapsed-game-seconds error including boundary uncertainty; default exact')
    parser.add_argument('--resume', action='store_true',
                        help='resume readiness or acknowledge completed output; never replay interrupted input')
    parser.add_argument('--ready-timeout', type=float, default=180)
    args = parser.parse_args()
    if args.timeout <= 0 or args.ready_timeout <= 0 or not 0 <= args.clock_tolerance < float("inf"):
        parser.error('timeout must be positive')
    execute(args)


if __name__ == '__main__':
    main()
