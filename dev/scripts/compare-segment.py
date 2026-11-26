#!/usr/bin/env python3
"""Run an explicit segment input tape in the original first, then cyrus.

Each executable-specific backend provides open_session(segment, output_directory), a context
manager yielding snapshot(), advance_to(relative_frame), and input(key, action). snapshot()
returns frame plus world/player/actors/flags/prompts/timing/continuations, in the same units and coordinate space.
advance_to must leave the game at the exact requested frame; skipped boundaries reject the run.
Script continuations include normalized cursors and all six task-slot states; a
limited import retaining native baseline tasks cannot establish a matched start.
Original executable bindings belong in an ignored backend, never this script.
"""
import argparse
import copy
from harness import exclusive_lock
import importlib.util
import json
import subprocess
from pathlib import Path

OBSERVABLES = ('world', 'player', 'actors', 'flags', 'prompts', 'timing', 'continuations')


class NativeFrameSession:
    """Drive a staged native control connection while ReplayHold keeps it stopped.

    project(state, scripts) normalizes the measured native state. Input is latched
    while ReplayHold is active, before the next observation-only schedule. All actors
    must fit in actor_limit; silently truncating the comparison is an error.
    Recorded scales are checked against the observed consumed clock step, with
    full continuations retained in clock_observations. Optional bios_ticks carries
    measured per-update BIOS intervals; omitting it uses diagnostic simulated BIOS
    timing and cannot establish complete original/native clock equivalence.
    """

    def __init__(self, control, project, actor_limit=4096, frame_scales=None, bios_ticks=None):
        self.control, self.project, self.actor_limit = control, project, actor_limit
        self.frame_scales = None if frame_scales is None else tuple(frame_scales)
        if self.frame_scales is not None and any(type(scale) is not int or not 0 <= scale <= 2**32 - 1 for scale in self.frame_scales):
            raise ValueError('recorded clock tape requires nonnegative integer Q8 frame scales')
        self.bios_ticks = None if bios_ticks is None else tuple(bios_ticks)
        if self.bios_ticks is not None and (
                self.frame_scales is None or len(self.bios_ticks) != len(self.frame_scales)
                or any(type(tick) is not int or not 0 <= tick <= 2**32 - 1 for tick in self.bios_ticks)):
            raise ValueError('recorded BIOS intervals require one unsigned interval per clock scale')
        self.current = self._read()
        self.origin = self.current[0]['scripts']['ordinary_updates']
        self.clock_observations = []
        if self.frame_scales is not None:
            self.clock_observations.append(self._clock_observation(0))

    def _read(self):
        state = self.control.request({'type': 'state'})
        scripts = self.control.request({'type': 'scripts', 'limit': self.actor_limit})
        after = self.control.request({'type': 'state'})
        if state['scripts']['ordinary_updates'] != after['scripts']['ordinary_updates']:
            raise RuntimeError('native simulation advanced outside an explicit frame step')
        if state.get('clock') != after.get('clock'):
            raise RuntimeError('native clock changed outside an explicit frame step')
        if after['scripts']['failed']:
            raise RuntimeError(f'native script failures: {after["scripts"]}')
        if len(scripts['actors']) != scripts['total']:
            raise RuntimeError('native comparison actor read was truncated')
        return after, scripts

    def _clock_observation(self, elapsed_scale):
        continuation = self.current[0].get('clock', {}).get('continuation')
        if not isinstance(continuation, dict):
            raise RuntimeError('recorded native clock continuation unavailable')
        return dict(frame=self.current[0]['scripts']['ordinary_updates'] - self.origin,
                    elapsed_scale=elapsed_scale, continuation=copy.deepcopy(continuation))

    def snapshot(self):
        state, scripts = self.current
        return dict(self.project(state, scripts), frame=state['scripts']['ordinary_updates'] - self.origin)

    def input(self, key, action):
        if action not in ('press', 'release'):
            raise ValueError(f'unsupported native key action {action}')
        before = self.snapshot()['frame']
        self.control.request(dict(type='input', key=key, action=action))
        self.current = self._read()
        if self.snapshot()['frame'] != before:
            raise RuntimeError('native frame changed during key acknowledgement')
        return self.snapshot()['frame']

    def advance_to(self, frame):
        current = self.snapshot()['frame']
        if type(frame) is not int or frame < current:
            raise ValueError(f'native frame {frame} already passed or invalid')
        if self.frame_scales is not None and frame > len(self.frame_scales):
            raise ValueError('recorded clock tape exhausted')
        while current < frame:
            command = ({'type': 'replay_frame', 'frame_scale': self.frame_scales[current]}
                       if self.frame_scales is not None else
                       {'type': 'schedule_input', 'events': [{'after_updates': 0}]})
            if self.bios_ticks is not None:
                command['elapsed_bios_ticks'] = self.bios_ticks[current]
            self.control.request(command)
            self.current = self._read()
            observed = self.snapshot()['frame']
            if observed != current + 1:
                raise RuntimeError(f'native frame gap: expected {current + 1}, observed {observed}')
            if self.frame_scales is not None:
                observation = self._clock_observation(self.clock_observations[-1]['elapsed_scale'])
                consumed = observation['continuation'].get('step', {}).get('frame_scale')
                if type(consumed) is not int or not 0 <= consumed <= 2**32 - 1:
                    raise RuntimeError('native consumed clock scale unavailable')
                observation['elapsed_scale'] += consumed
                observation['requested_scale'] = self.frame_scales[current]
                self.clock_observations.append(observation)
                if consumed != self.frame_scales[current]:
                    raise RuntimeError(f'native consumed clock scale {consumed}, requested {self.frame_scales[current]}')
            current = observed


def input_tape(segment):
    """Compile input-only replay steps; never silently discard steering or dialogue decisions."""
    events, checkpoints, cursor = [], [0], 0
    for index, step in enumerate(segment['steps']):
        if set(step) == {'input'}:
            batch = step['input']
            if not batch:
                raise ValueError(f'step {index}: empty input batch')
            for event in batch:
                offset = event.get('after_updates')
                if type(offset) is not int or offset < 0:
                    raise ValueError(f'step {index}: invalid frame offset')
                if 'key' in event or 'action' in event:
                    if not isinstance(event.get('key'), str) or event.get('action') not in ('press', 'release'):
                        raise ValueError(f'step {index}: input needs key and press/release action')
                    events.append(dict(frame=cursor + offset, key=event['key'], action=event['action']))
            # The runner consumes the final boundary's update as well.
            cursor += max(event['after_updates'] for event in batch) + 1
            checkpoints.append(cursor)
        elif set(step) == {'checkpoint'}:
            checkpoints.append(cursor)
        else:
            raise ValueError(f'step {index}: supply expanded input steps; unsupported {list(step)}')
    if not events:
        raise ValueError('segment has no key input')
    return sorted(events, key=lambda event: event['frame']), sorted(set(checkpoints))


def delta(original, native, path=''):
    """Keep both measured values; report missing fields and numeric signed deltas explicitly."""
    if isinstance(original, dict) and isinstance(native, dict):
        rows = []
        for key in sorted(original.keys() | native.keys()):
            name = f'{path}.{key}' if path else key
            if key not in original or key not in native:
                rows.append(dict(field=name, status='unavailable', original=original.get(key), native=native.get(key)))
            else:
                rows.extend(delta(original[key], native[key], name))
        return rows
    if original is None or native is None:
        return [dict(field=path, status='unavailable', original=original, native=native)]
    if isinstance(original, list) and isinstance(native, list) and len(original) == len(native):
        return [row for index, (a, b) in enumerate(zip(original, native))
                for row in delta(a, b, f'{path}[{index}]')]
    row = dict(field=path, status='match' if type(original) is type(native) and original == native else 'different',
               original=original, native=native)
    if type(original) in (int, float) and type(native) in (int, float):
        row['delta'] = native - original
    return [row]


def snapshot(session, frame):
    state = session.snapshot()
    if type(state.get('frame')) is not int or state['frame'] != frame:
        raise ValueError(f'frame boundary missed: requested {frame}, observed {state.get("frame")}')
    for field in OBSERVABLES:
        if field not in state:
            raise ValueError(f'frame {frame}: missing observable {field}')
    return state


def record(session, events, checkpoints):
    """Yield each measured checkpoint before sending inputs at that boundary.

    The caller retains completed observations immediately, including when a later
    boundary fails, and can stop native execution at the first actual mismatch.
    """
    yield snapshot(session, 0)
    boundaries = sorted(set(checkpoints) | {event['frame'] for event in events})
    for frame in boundaries:
        if frame:
            session.advance_to(frame)
            state = snapshot(session, frame)
            if frame in checkpoints:
                yield state
        for event in events:
            if event['frame'] == frame:
                applied = session.input(event['key'], event['action'])
                if type(applied) is not int or applied != frame:
                    raise ValueError(f'input frame missed: requested {frame}, acknowledged {applied}')


def run_pair(segment, original, native, output):
    report = dict(version=1, status='blocked', events=[], checkpoints=[])
    try:
        events, checkpoints = input_tape(segment)
        report['events'] = events
        (output / 'original').mkdir(parents=True, exist_ok=True)
        (output / 'native').mkdir(parents=True, exist_ok=True)
        report['original'] = []
        with original(segment, output / 'original') as session:
            for observation in record(session, events, checkpoints):
                report['original'].append(observation)
        report['native'] = []
        with native(segment, output / 'native') as session:
            for expected, measured in zip(report['original'], record(session, events, checkpoints)):
                report['native'].append(measured)
                fields = delta({key: expected[key] for key in OBSERVABLES},
                               {key: measured[key] for key in OBSERVABLES})
                mismatch = any(row['status'] != 'match' for row in fields)
                if mismatch and measured['frame'] == 0:
                    report.update(reason='start states differ or are incomplete', start_deltas=fields)
                    return report
                report['checkpoints'].append(dict(frame=measured['frame'], fields=fields))
                if mismatch:
                    report['status'] = 'different'
                    return report
        report['status'] = 'match'
    except (ValueError, RuntimeError, TimeoutError, OSError) as error:
        report['reason'] = str(error)
    return report


def backend(path):
    spec = importlib.util.spec_from_file_location('comparison_backend', path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.open_session


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('segment')
    parser.add_argument('--original-backend', required=True)
    parser.add_argument('--native-backend', required=True)
    parser.add_argument('--out', required=True, help='host directory under magic/')
    args = parser.parse_args()
    output = Path(args.out)
    output.mkdir(parents=True, exist_ok=True)
    with exclusive_lock(output / 'comparison.lock'):
        subprocess.run([str(Path(__file__).with_name("ensure-retro-runtime.sh"))],
                       check=True, stdout=subprocess.DEVNULL)
        report = run_pair(json.loads(Path(args.segment).read_text()), backend(args.original_backend),
                          backend(args.native_backend), output)
        report['segment'] = str(args.segment)
        (output / 'comparison.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps(dict(status=report['status'], report=str(output / 'comparison.json'))))
    return 0 if report['status'] == 'match' else 1


if __name__ == '__main__':
    raise SystemExit(main())
