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


def planned_updates(expected, step_seconds, tolerance=0):
    """Refuse an unreachable exact interval before sending gameplay input."""
    target = sum(expected['duration']) / 2
    count = max(1, round(target / step_seconds))
    try:
        compare_interval(expected, dict(duration=[count * step_seconds] * 2), tolerance)
    except ValueError as error:
        from fractions import Fraction
        import math
        original = Fraction(target).limit_denominator(1_000_000)
        native = Fraction(step_seconds).limit_denominator(1_000_000)
        denominator = math.lcm(original.denominator, native.denominator)
        common = Fraction(math.lcm(int(original * denominator), int(native * denominator)), denominator)
        raise ValueError(f'interval unavailable at measured ordinary step; reacquire original for '
                         f'candidate common interval {float(common)} seconds; actual clocks still required') from error
    return count


def valid_baseline(player, attachment):
    weapon_matches = not player['drawn'] if attachment else bool(player['drawn'])
    motion_matches = attachment or not player['falling']
    return weapon_matches and motion_matches and not player['living'] and player['health'] > 0


def attachment_outcome(phase, start, end):
    expected = {'approach': (False, True), 'hang': (True, True), 'detach': (True, False)}
    if phase not in expected or (start.get('rope_attached'), end.get('rope_attached')) != expected[phase]:
        raise ValueError(f'positive rope {phase} attachment outcome was rejected')


def validate_physical_source(rows):
    """Revalidate frozen acquisition evidence before admitting physical replay."""
    spec = importlib.util.spec_from_file_location('original_rope', Path(__file__).with_name('original-rope-contact.py'))
    original = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(original)
    for phase in ('approach', 'hang', 'detach'):
        records = [row for row in rows if row.get('phase') == 'schedule-evidence'
                   and row.get('request', {}).get('phase') == phase]
        if len(records) != 1 or records[0]['request'].get('atomic_snapshots') is not True:
            raise ValueError('physical replay requires atomic snapshot evidence')
        record = records[0]
        validated = original.validate_schedule(record['result'], record['request'])
        published = [row for row in rows if row.get('phase') == phase and row.get('boundary')]
        if published != validated:
            raise ValueError('physical replay boundaries differ from atomic snapshot evidence')


def accepted_schedule(path, attachment=False):
    rows = [json.loads(line) for line in path.read_text().splitlines()]
    names = ('approach', 'hang', 'detach') if attachment else ('walk-in', 'attack', 'continuation')
    phases = {}
    for row in rows:
        if row['phase'] in ('placed', *names):
            player = row['player']
            if not valid_baseline(player, attachment):
                raise ValueError('original contact baseline was rejected')
            phases.setdefault(row['phase'], []).append(row)
    if set(phases) != {'placed', *names}:
        raise ValueError('original contact run is incomplete')
    physical = [i for i, row in enumerate(rows) if row.get('phase') == 'physical-release']
    if physical:
        hang = [i for i, row in enumerate(rows) if row.get('phase') == 'hang' and row.get('boundary') == 'end']
        detach = [i for i, row in enumerate(rows) if row.get('phase') == 'detach' and row.get('boundary') == 'start']
        if (not attachment or len(physical) != 1 or len(hang) != 1 or len(detach) != 1
                or not hang[0] < physical[0] < detach[0]):
            raise ValueError('physical release must follow hanging and precede detach')
        player = rows[physical[0]]['player']
        if not valid_baseline(player, True) or not player.get('rope_attached'):
            raise ValueError('physical release requires a retained attachment')
    schedule = {}
    for phase in names:
        edges = [row for row in phases[phase] if row.get('boundary')]
        boundaries = {row['boundary']: row for row in edges}
        expected_edges = {'start', 'release', 'end'} if physical and phase == 'detach' and 'release' in boundaries else {'start', 'end'}
        if len(edges) != len(expected_edges) or set(boundaries) != expected_edges:
            raise ValueError(f'original {phase} lacks acknowledged game-clock boundaries; acquire timing samples')
        if not all(row.get('input_ack') is True for row in boundaries.values()):
            raise ValueError(f'original {phase} input was not acknowledged')
        if attachment:
            attachment_outcome(phase, boundaries['start']['player'], boundaries['end']['player'])
        schedule[phase] = interval(boundaries['start'], boundaries['end'])
        if 'release' in boundaries:
            schedule[phase]['pulse'] = interval(boundaries['start'], boundaries['release'])
            schedule[phase]['supported_landing'] = True
    if physical:
        validate_physical_source(rows)
        schedule['detach']['physical_release'] = True
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
        self.attachment = False

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
                if not valid_baseline(player, self.attachment):
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
        updates = planned_updates(expected, step_seconds, self.clock_tolerance)
        pulse = expected.get('pulse')
        release_updates = planned_updates(pulse, step_seconds, self.clock_tolerance) if pulse else updates
        if pulse and not 0 < release_updates < updates:
            raise ValueError('native pulse requires a released continuation')
        events = [dict(after_updates=offset, **(
            dict(key=key, action=action) if key else {}))
            for offset, action in [(0, 'press'), (release_updates, 'release')]]
        if pulse:
            events += [dict(after_updates=i) for i in range(1, updates + 1) if i != release_updates]
            events.sort(key=lambda event: event['after_updates'])
        response = self.call('schedule_input', events=events)
        rows = response['boundaries']
        if len(rows) != len(events):
            raise RuntimeError('native schedule did not report both boundaries')
        boundaries = []
        for row, event in zip(rows, events):
            edge = ('start' if event['after_updates'] == 0 else
                    'end' if event['after_updates'] == updates else
                    'release' if event['after_updates'] == release_updates else 'sample')
            if (row['after_updates'] != event['after_updates']
                    or row.get('key') != event.get('key')
                    or row.get('action') != event.get('action')):
                raise RuntimeError('native boundary acknowledgement differs from schedule')
            if rows and (row['update'] != rows[0]['update'] + event['after_updates']
                    or row['generation'] != rows[0]['generation']):
                raise RuntimeError('native schedule crossed unavailable ordinary boundaries')
            actor, = row['player']['actors']
            control = actor['control']
            player = dict(drawn=control['weapon_drawn'], falling=control['falling_state'],
                living=control['living_state'], health=actor['attributes'][3],
                rope_attached=control['rope_attached'], queued=control.get('queued_motion'),
                group=actor.get('animation', {}).get('current_group'),
                position=[v / 256 for v in actor['spatial']['effective']['position']])
            boundary = dict(phase=phase, boundary=edge, input_ack=True,
                frame=row['update'], generation=row['generation'],
                clock=[row['game_seconds'], row['game_seconds']], player=player)
            self.output.write(json.dumps(boundary) + '\n')
            self.output.flush()
            if not valid_baseline(player, self.attachment):
                raise RuntimeError('native contact baseline was rejected')
            boundaries.append(boundary)
        start, end = boundaries[0], boundaries[-1]
        if (start['generation'] != end['generation']
                or end['frame'] - start['frame'] != updates):
            raise RuntimeError('native schedule crossed unavailable ordinary boundaries')
        if pulse:
            released = next(row for row in boundaries if row['boundary'] == 'release')
            if (released['generation'] != start['generation']
                    or released['frame'] - start['frame'] != release_updates):
                raise RuntimeError('native pulse release crossed unavailable ordinary boundaries')
            compare_interval(pulse, interval(start, released), self.clock_tolerance)
        observed = interval(start, end)
        try:
            compare_interval(expected, observed, self.clock_tolerance)
        except ValueError as error:
            self.output.write(json.dumps(dict(phase=phase, comparison='rejected',
                duration=observed['duration'], original_duration=expected['duration'],
                clock_tolerance=self.clock_tolerance, error=str(error))) + '\n')
            self.output.flush()
            raise
        if self.attachment:
            attachment_outcome(phase, start['player'], end['player'])
        if expected.get('supported_landing'):
            detached = False
            for row in boundaries:
                attached = row['player']['rope_attached']
                if detached and attached:
                    raise ValueError('native pulse reattached during landing continuation')
                detached |= attached is False
            player = end['player']
            if (player['rope_attached'] or player['falling'] != 0 or player.get('queued') != 0
                    or not any(row['player'].get('group') in range(65, 75) for row in boundaries)):
                raise ValueError('native pulse did not reach supported landing')
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
            if not self.attachment:
                self.call('console', line='item add,1')
            if not self.attachment and not self.sample('inventory')['player']['drawn']:
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
                phases = [('approach', 'Space'), ('hang', None), ('detach', 'Space')] if self.attachment else [
                    ('walk-in', 'ArrowUp'), ('attack', 'ControlLeft'), ('continuation', None)]
                for phase, key in phases:
                    if phase == 'detach' and schedule[phase].get('physical_release'):
                        self.call('cheat', name='magiccarpet', value=0)
                        row = self.sample('physical-release')
                        if not valid_baseline(row['player'], True) or not row['player'].get('rope_attached'):
                            raise RuntimeError('physical release requires a retained attachment')
                    self.timed_segment(phase, schedule[phase], key)
        finally:
            for key in tuple(self.held):
                self.key(key, False)


def execute(args):
    original, schedule = accepted_schedule(args.original, attachment=args.attachment)
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
    driver.attachment = args.attachment
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
    parser.add_argument('--attachment', action='store_true',
                        help='replay and require positive unarmed grab, hold and jump detach')
    parser.add_argument('--resume', action='store_true',
                        help='resume readiness or acknowledge completed output; never replay interrupted input')
    parser.add_argument('--ready-timeout', type=float, default=180)
    args = parser.parse_args()
    if args.timeout <= 0 or args.ready_timeout <= 0 or not 0 <= args.clock_tolerance < float("inf"):
        parser.error('timeout must be positive')
    execute(args)


if __name__ == '__main__':
    main()
