#!/usr/bin/env python3
"""Run an explicit segment input tape in the original first, then cyrus.

Each executable-specific backend provides open_session(segment, output_directory), a context
manager yielding snapshot(), advance_to(relative_frame), and input(key, action). snapshot()
returns frame plus world/player/actors/flags/prompts/timing, in the same units and coordinate space.
advance_to must leave the game at the exact requested frame; skipped boundaries reject the run.
Original executable bindings belong in an ignored backend, never this script.
"""
import argparse
import fcntl
import importlib.util
import json
import subprocess
from pathlib import Path

OBSERVABLES = ('world', 'player', 'actors', 'flags', 'prompts', 'timing')


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


def record(session, events, checkpoints, baseline=None):
    first = snapshot(session, 0)
    if baseline is not None:
        differences = delta({key: baseline[key] for key in OBSERVABLES},
                            {key: first[key] for key in OBSERVABLES})
        if any(row['status'] != 'match' for row in differences):
            return [first], differences
    rows = [first]
    boundaries = sorted(set(checkpoints) | {event['frame'] for event in events})
    for frame in boundaries:
        if frame:
            session.advance_to(frame)
            state = snapshot(session, frame)
            if frame in checkpoints:
                rows.append(state)
        for event in events:
            if event['frame'] == frame:
                applied = session.input(event['key'], event['action'])
                if type(applied) is not int or applied != frame:
                    raise ValueError(f'input frame missed: requested {frame}, acknowledged {applied}')
    return rows, []


def run_pair(segment, original, native, output):
    report = dict(version=1, status='blocked', events=[], checkpoints=[])
    try:
        events, checkpoints = input_tape(segment)
        report['events'] = events
        (output / 'original').mkdir(parents=True, exist_ok=True)
        (output / 'native').mkdir(parents=True, exist_ok=True)
        with original(segment, output / 'original') as session:
            reference, _ = record(session, events, checkpoints)
        report['original'] = reference
        with native(segment, output / 'native') as session:
            candidate, mismatch = record(session, events, checkpoints, reference[0])
        report['native'] = candidate
        if mismatch:
            report.update(reason='start states differ or are incomplete', start_deltas=mismatch)
            return report
        for a, b in zip(reference, candidate):
            fields = delta({key: a[key] for key in OBSERVABLES}, {key: b[key] for key in OBSERVABLES})
            report['checkpoints'].append(dict(frame=a['frame'], fields=fields))
        report['status'] = 'match' if all(row['status'] == 'match' for point in report['checkpoints']
                                        for row in point['fields']) else 'different'
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
    with (output / 'comparison.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
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
