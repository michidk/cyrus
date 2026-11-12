#!/usr/bin/env python3
"""Capture a pre-input baseline and every boundary of a bounded edge attempt.

Start from an externally established grounded pose. No placement or asynchronous
input occurs here. Raw responses survive rejection; acceptance requires the
complete declared schedule, falling entry and a subsequent health endpoint.
"""
import argparse
import importlib.util
import json
import os
from pathlib import Path

spec = importlib.util.spec_from_file_location('trace', Path(__file__).with_name('native-trace.py'))
trace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trace)


def schedule(key, press, release, updates):
    if not 0 < press < release < updates <= 255:
        raise ValueError('require 0 < press < release < updates <= 255')
    events = [dict(after_updates=i) for i in range(updates + 1)]
    events[press].update(key=key, action='press')
    events[release].update(key=key, action='release')
    return events


def validate(response, events):
    boundaries = response.get('boundaries', [])
    if len(boundaries) != len(events):
        raise ValueError('incomplete edge schedule')
    rows = []
    for boundary, event in zip(boundaries, events):
        if boundary.get('after_updates') != event['after_updates']:
            raise ValueError('missing or reordered edge boundary')
        row = trace.boundary_sample(boundary, 'edge', (event.get('key'), event.get('action')))
        if any(type(row[field]) is not int for field in ('health', 'falling', 'queued', 'sliding', 'living')):
            raise ValueError('selected edge state unavailable')
        if rows:
            trace.validate_continuation(rows[-1], row)
            if row['frame'] != rows[-1]['frame'] + 1:
                raise ValueError('sampling gap invalidates exact edge acceptance')
        rows.append(row)
    baseline = rows[0]
    if (baseline['falling'] != 0 or baseline['health'] is None or baseline['health'] <= 0
            or any(baseline[field] != 0 for field in ('queued', 'sliding', 'living'))):
        raise ValueError('edge baseline must be alive, ordinary and nonfalling before input')
    press = next(i for i, event in enumerate(events) if event.get('action') == 'press')
    if any(row['falling'] != 0 or row['health'] != baseline['health'] for row in rows[:press + 1]):
        raise ValueError('edge transition preceded acknowledged trigger')
    falling = next((i for i in range(press + 1, len(rows)) if rows[i]['falling'] != 0), None)
    if falling is None:
        raise ValueError('edge falling entry not captured')
    if not any(row['falling'] == 0 and row['health'] is not None
               and row['health'] < baseline['health']
               and rows[i + 1]['falling'] == 0 and rows[i + 1]['health'] == row['health']
               for i, row in enumerate(rows[:-1]) if i > falling):
        raise ValueError('post-edge health/fatal endpoint not captured')
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__.split('\n')[0])
    parser.add_argument('--addr', default=os.environ.get('CYRUS_CONTROL_ADDR', '127.0.0.1:7167'))
    parser.add_argument('--out', required=True)
    parser.add_argument('--key', default='ArrowUp')
    parser.add_argument('--press-update', type=int, default=1)
    parser.add_argument('--release-update', type=int, required=True)
    parser.add_argument('--updates', type=int, required=True)
    args = parser.parse_args()
    events = schedule(args.key, args.press_update, args.release_update, args.updates)
    Path(args.out).unlink(missing_ok=True)
    Path(args.out + '.raw.json').unlink(missing_ok=True)
    trace.wait_live(args.addr, 180)
    response = trace.call(args.addr, dict(type='schedule_input', events=events))
    # Preserve rejected acquisition too, without presenting it as accepted JSONL.
    Path(args.out + '.raw.json').write_text(json.dumps(response) + '\n')
    rows = validate(response, events)
    Path(args.out).write_text(''.join(json.dumps(row) + '\n' for row in rows))


if __name__ == '__main__':
    main()
