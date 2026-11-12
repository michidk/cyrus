#!/usr/bin/env python3
"""Measure installed speech retirement and replacement skip continuity in a paced runtime.

The speech_teardown example owns public ScriptPlugin scene replacement. The
runtime must bind-mount the selected output directory at the path passed to the example.
No preview or physical audio device is used.
"""
import argparse
import fcntl
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import time

spec = importlib.util.spec_from_file_location('ambient', Path(__file__).with_name('native-ambient-lifecycle.py'))
ambient = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ambient)


def wait_for_executable(container, expected, seconds=45):
    deadline = time.monotonic() + seconds
    while True:
        processes = subprocess.run(['docker', 'exec', container, 'bash', '-c',
            'sha256sum /proc/[0-9]*/exe 2>/dev/null'], capture_output=True, text=True, timeout=10)
        count = sum(line.split()[0] == expected for line in processes.stdout.splitlines() if line.split())
        if count == 1:
            break
        if count > 1 or time.monotonic() > deadline:
            raise ValueError('expected exactly one running probe matching executable SHA256')
        time.sleep(.05)


def validate_output(output):
    if any(output.glob('phase-*.json')):
        raise ValueError('output contains prior stage receipts; use a fresh output directory')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('container')
    parser.add_argument('output', type=Path)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with (args.output / 'acceptance.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        validate_output(args.output)
        expected = hashlib.sha256(args.binary.read_bytes()).hexdigest()
        wait_for_executable(args.container, expected)
        result = {'executable': expected, 'stages': []}
        try:
            for phase, present in enumerate([True, False, False, True, True, False]):
                (args.output / 'command').write_text(str(phase))
                receipt = args.output / f'phase-{phase}.json'
                deadline = time.monotonic() + 45
                while not receipt.exists():
                    if time.monotonic() > deadline:
                        raise TimeoutError(f'phase {phase} did not complete its public update')
                    time.sleep(.05)
                report = json.loads(receipt.read_text())
                if report.get('phase') != phase:
                    raise ValueError('stage receipt does not match the requested public update')
                time.sleep(.2)  # Drain already mixed output before testing explicit retirement.
                captured = subprocess.run(['docker', 'exec', '-e', 'PULSE_SERVER=unix:/run/pulse/native',
                    args.container, 'timeout', '10', 'bash', '-c',
                    'parecord --raw --format=s16le --rate=22050 --channels=2 '
                    '--latency-msec=50 --device=cyrus.monitor | head -c 88200'],
                    capture_output=True, timeout=15)
                if captured.returncode != 0:
                    raise RuntimeError(captured.stderr.decode())
                (args.output / f'phase-{phase}.pcm').write_bytes(captured.stdout)
                levels = ambient.pcm_levels(captured.stdout)
                result['stages'].append({**report, 'rms': levels, 'expected_signal': present})
                (args.output / 'progress.json').write_text(json.dumps(result, indent=2)+'\n')
                ambient.assert_signal(levels, present)
                if report['playing'] != int(present):
                    raise ValueError(f'phase {phase} unexpected sample ownership: {report}')
                if phase == 3 and report['players'] == result['stages'][0]['players']:
                    raise ValueError('replacement voice reused the retired playback entity')
                if phase == 4 and report['players'] != result['stages'][3]['players']:
                    raise ValueError('skip replaced the continuing voice owner')
                if phase == 0:
                    if report['dialogue']['presented'] is None:
                        raise ValueError('active installed voice did not publish its subtitle')
                elif phase == 1:
                    if report['generation'] is not None or report['dialogue'] is not None:
                        raise ValueError('released scene retained a logical world')
                else:
                    dialogue = report['dialogue']
                    if dialogue['presented'] is not None or any(record['active'] or record['text'] or (phase != 3 and record['sample'] is not None) for record in dialogue['records']):
                        raise ValueError('world replacement retained dialogue ownership')
            (args.output / 'result.json').write_text(json.dumps(result, indent=2)+'\n')
            print(json.dumps(result, indent=2))
        finally:
            (args.output / 'command').write_text('6')


if __name__ == '__main__':
    main()
