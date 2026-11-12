#!/usr/bin/env python3
"""Observe authored ambient audio and shared-pool retirement through public scene loads.

Run in a short paced-audio runtime with host-mounted config. The caller selects
an installed emitter and loop effect explicitly; no installed data is embedded.
"""
import argparse
import array
import fcntl
import importlib.util
import json
import math
from pathlib import Path
import subprocess
import sys

spec = importlib.util.spec_from_file_location('capture', Path(__file__).with_name('native-session-capture.py'))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


def pcm_levels(data):
    values = array.array('h')
    values.frombytes(data)
    if sys.byteorder != 'little':
        values.byteswap()
    if len(values) < 22050:
        raise ValueError('incomplete monitor window')
    return [math.sqrt(sum(v*v for v in values[channel::2]) / len(values[channel::2]))
            for channel in range(2)]


def assert_signal(levels, present):
    mismatch = max(levels) <= 8 if present else max(levels) > 2
    if mismatch:
        raise ValueError(f'PCM presence {present} differs: {levels}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('port', type=int)
    parser.add_argument('output', type=Path)
    parser.add_argument('--container', required=True)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--emitter', required=True)
    parser.add_argument('--effect', type=int, required=True)
    parser.add_argument('--scene', default='ISLAND')
    parser.add_argument('--destination', default='START')
    parser.add_argument('--seconds', type=float, default=90)
    parser.add_argument('--prepare-seconds', type=float, default=600)
    parser.add_argument('--game-seconds', type=float, default=.4)
    parser.add_argument('--interval', type=float, default=.2)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with (args.output / 'ambient.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        driver = capture.Driver(args)
        actual = capture.file_digest(args.binary)
        results = {'executable': actual, 'stages': {}}

        def record(name, present):
            driver.phase(name)
            result = subprocess.run(['docker', 'exec', '-e', 'PULSE_SERVER=unix:/run/pulse/native',
                args.container, 'timeout', '15', 'bash', '-c',
                'parecord --raw --format=s16le --rate=22050 --channels=2 '
                '--latency-msec=50 --device=cyrus.monitor | head -c 176400'],
                capture_output=True, timeout=20)
            if result.returncode != 0:
                raise RuntimeError(result.stderr.decode())
            capture.atomic(args.output / (name + '.pcm'), result.stdout)
            levels = pcm_levels(result.stdout[len(result.stdout)//2//4*4:])
            results['stages'][name] = {'rms': levels, 'present': present, 'state': driver.request('state')}
            capture.atomic(args.output / 'progress.json', json.dumps(results, indent=2).encode())
            assert_signal(levels, present)

        def ready(scene):
            driver.ready(scene)
            return driver.wait(lambda: (state if (state := driver.request('state'))['scripts'].get('state') == 'Loaded' else None), args.prepare_seconds)

        try:
            driver.progress(ready(args.scene))
            processes = subprocess.run(['docker', 'exec', args.container, 'bash', '-c',
                'sha256sum /proc/[0-9]*/exe 2>/dev/null'], capture_output=True, text=True, timeout=30)
            running = [line.split()[0] for line in processes.stdout.splitlines() if line.split()]
            if running.count(actual) != 1:
                raise ValueError('expected exactly one running game with the supplied executable digest')
            state = driver.request('state')
            emitter = None
            for offset in range(0, state['scripts']['instances'], 64):
                rows = driver.request('scripts', offset=offset, limit=64)['actors']
                emitter = next((row for row in rows if row['name'] == args.emitter), None)
                if emitter:
                    break
            if not emitter:
                raise ValueError('selected installed emitter absent')
            capture.atomic(args.output / 'emitter.json', json.dumps(emitter, indent=2).encode())
            point = [v // 256 for v in emitter['spatial']['effective']['position']]
            for name, dx in [('room-near', 0), ('room-moved', 512)]:
                driver.request('place_player', x=point[0]+dx, y=point[1], z=point[2], heading=0)
                driver.request('reset_camera')
                driver.request('console', open=False)
                driver.progress(ready(args.scene))
                driver.request('console', open=True)
                record(name, True)
            driver.request('stop_sounds')
            record('stopped', False)
            driver.request('play_sound', effect=args.effect)
            record('unowned-start', True)
            driver.request('load_scene', name=args.destination)
            ready(args.destination)
            record('retired', False)
            driver.request('play_sound', effect=args.effect)
            record('replacement-start', True)
            driver.request('stop_sounds')
            record('replacement-stop', False)
            driver.request('load_scene', name=args.scene)
            driver.request('console', open=False)
            driver.progress(ready(args.scene))
            driver.request('place_player', x=point[0], y=point[1], z=point[2], heading=0)
            driver.request('reset_camera')
            driver.request('console', open=False)
            driver.progress(ready(args.scene))
            driver.request('console', open=True)
            record('revisited', True)
            capture.atomic(args.output / 'result.json', json.dumps(results, indent=2).encode())
        finally:
            driver.request('console', open=False)


if __name__ == '__main__':
    main()
