#!/usr/bin/env python3
"""Exercise public global sample stop and continuation with a real Pulse monitor.

Run against a paced-audio container with host-mounted config/output. The install
is supplied by the caller; no sample data or captured PCM belongs in git.
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


def energy(path):
    data = array.array('h')
    data.frombytes(path.read_bytes())
    if sys.byteorder != 'little':
        data.byteswap()
    if len(data) < 22050:
        raise ValueError('native monitor did not capture a sufficient PCM window')
    return math.sqrt(sum(value * value for value in data) / len(data))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('port', type=int)
    parser.add_argument('output', type=Path)
    parser.add_argument('--container', required=True)
    parser.add_argument('--effect', type=int, default=2)
    parser.add_argument('--seconds', type=float, default=180)
    parser.add_argument('--prepare-seconds', type=float, default=600)
    parser.add_argument('--game-seconds', type=float, default=.4)
    parser.add_argument('--interval', type=float, default=.1)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with (args.output / 'sound-reset.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        driver = capture.Driver(args)
        driver.progress(driver.ready())
        driver.request('console', open=True)
        results = {}
        def record(name):
            path = args.output / (name + '.pcm')
            with path.open('wb') as stream:
                result = subprocess.run(['docker', 'exec', '-e', 'PULSE_SERVER=unix:/run/pulse/native',
                    args.container, 'timeout', '2', 'parecord', '--raw', '--format=s16le',
                    '--rate=22050', '--channels=2', '--device=cyrus.monitor'], stdout=stream,
                    stderr=subprocess.PIPE, timeout=10)
            if result.returncode not in (0, 124):
                raise RuntimeError(result.stderr.decode())
            results[name] = {'rms': energy(path), 'sha256': capture.file_digest(path)}
        try:
            driver.request('stop_sounds')
            driver.wait(lambda: driver.request('state')['sounds']['playing'] == 0, args.seconds)
            record('baseline')
            for name in ('started', 'continued'):
                driver.request('play_sound', effect=args.effect)
                driver.wait(lambda: driver.request('state')['sounds']['playing'] > 0, args.seconds)
                record(name)
                driver.request('stop_sounds')
                driver.wait(lambda: driver.request('state')['sounds']['playing'] == 0, args.seconds)
                record(name + '-stopped')
            floor = max(2, results['baseline']['rms'] * 2)
            for name in ('started', 'continued'):
                if results[name]['rms'] <= max(8, floor * 4):
                    raise ValueError(f'{name}: no audible monitor signal')
                if results[name + '-stopped']['rms'] > floor:
                    raise ValueError(f'{name}: stopped sample still audible')
            capture.atomic(args.output / 'result.json', json.dumps(results, indent=2).encode())
        finally:
            driver.request('console', open=False)
        driver.progress(driver.ready())


if __name__ == '__main__':
    main()
