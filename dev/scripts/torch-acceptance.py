#!/usr/bin/env python3
"""Check goldfinger through a ready native START world's public control port.

Uses set_torch to seed the counter, bypassing lighting presentation. The ordinary
world updates must then count it down with goldfinger off and refresh it to 100
with goldfinger on. This does not verify torch insertion, artwork or lighting.
Print the JSONL record into an ignored artifact directory.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import time

spec = importlib.util.spec_from_file_location('native_trace', Path(__file__).with_name('native-trace.py'))
trace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trace)


def verify(counters, retained):
    """Require multiple observations of countdown or a continued full counter."""
    assert len(counters) >= 3, 'insufficient continuation samples'
    if retained:
        assert all(value == 100 for value in counters), f'torch was not retained: {counters}'
    else:
        assert all(0 < value <= 73 for value in counters), f'invalid countdown: {counters}'
        assert all(a >= b for a, b in zip(counters, counters[1:])), 'counter increased without goldfinger'
        assert counters[-1] < counters[0], 'torch did not count down'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--addr', default='127.0.0.1:7167')
    parser.add_argument('--timeout', type=float, default=120)
    args = parser.parse_args()
    trace.wait_live(args.addr, args.timeout)
    def call(command):
        return trace.call(args.addr, command)
    state = call({'type': 'state'})
    assert state['scene'] == 'START', 'run a fresh START world on dry ground'
    index = trace.player_index(args.addr)
    original = next(entry['value'] for entry in state['cheats']['entries'] if entry['name'] == 'goldfinger')
    actor = call({'type': 'scripts', 'offset': index, 'limit': 1})['actors'][0]
    original_torch = actor['control']['torch_burning']
    try:
        for retained in (False, True):
            call({'type': 'cheat', 'name': 'goldfinger', 'value': int(retained)})
            call({'type': 'set_torch', 'burning': 73})
            counters = []
            last = None
            start = None
            deadline = time.monotonic() + args.timeout
            while len(counters) < 4:
                assert time.monotonic() < deadline, 'torch continuation timed out'
                state = call({'type': 'state'})
                assert not state['scripts'].get('error'), state['scripts']
                assert state['scripts'].get('failed', 0) == 0, state['scripts']
                actor = call({'type': 'scripts', 'offset': index, 'limit': 1})['actors'][0]
                assert not actor.get('failure'), actor.get('failure')
                now = state['clock']['game_seconds']
                if start is None:
                    start = now
                # Allow the modulo-eight refresh to run, then sample continued game time.
                if now - start >= 2 and (last is None or now - last >= 0.5):
                    counter = actor['control']['torch_burning']
                    counters.append(counter)
                    last = now
                    print(json.dumps({'retained': retained, 'frame': state['frame'],
                                      'game_seconds': now, 'torch_burning': counter}), flush=True)
                time.sleep(0.02)
            verify(counters, retained)
        print(json.dumps({'accepted': True}), flush=True)
    finally:
        try:
            call({'type': 'set_torch', 'burning': original_torch})
        finally:
            call({'type': 'cheat', 'name': 'goldfinger', 'value': original})


if __name__ == '__main__':
    main()
