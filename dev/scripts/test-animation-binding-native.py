#!/usr/bin/env python3
"""Observe an isolated animation_binding example through a paced Pulse monitor."""
import argparse
import array
import fcntl
import json
from pathlib import Path
import subprocess
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('container')
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with (args.output / 'acceptance.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        results = []
        deadline = time.monotonic() + 85
        while len(results) < 7:
            if time.monotonic() > deadline:
                raise RuntimeError('bounded SOUP output acceptance expired')
            logs = subprocess.check_output(['docker', 'logs', args.container], stderr=subprocess.STDOUT, timeout=5).decode()
            stages = [json.loads(line) for line in logs.splitlines() if line.startswith('{') and 'animation_binding' in line]
            if len(stages) <= len(results):
                time.sleep(.05)
                continue
            stage = stages[len(results)]
            assert stage['phase'] == len(results)
            assert stage['expected_channels'] == [1, 0, 1, 0, 0, 1, 0][stage['phase']]
            time.sleep(.2)
            captured = subprocess.run(['docker', 'exec', '-e', 'PULSE_SERVER=unix:/run/pulse/native', args.container,
                'timeout', '2', 'parecord', '--raw', '--format=s16le', '--rate=22050', '--channels=2', '--latency-msec=50', '--device=cyrus.monitor'],
                capture_output=True, timeout=5)
            assert captured.returncode in (0, 124), captured.stderr
            samples = array.array('h', captured.stdout)
            assert len(samples) > 22050, 'insufficient actual output frames'
            energy = sum(x*x for x in samples) / len(samples)
            assert (energy > 64) if stage['expected_channels'] else (energy == 0), (stage, energy)
            results.append({**stage, 'pcm_samples': len(samples), 'mean_square': energy})
            (args.output / 'result.json').write_text(json.dumps({'results': results}, indent=2)+'\n')
        print(json.dumps({'results': results}, indent=2))


if __name__ == '__main__':
    main()
