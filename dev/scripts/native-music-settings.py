#!/usr/bin/env python3
"""Capture real paced mixer output while the music_settings public owner probe advances."""
import argparse
import fcntl
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import time

spec = importlib.util.spec_from_file_location('speech', Path(__file__).with_name('native-speech-teardown.py'))
speech = importlib.util.module_from_spec(spec)
spec.loader.exec_module(speech)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('container')
    parser.add_argument('output', type=Path)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--track', type=int, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with (args.output / 'acceptance.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        speech.validate_output(args.output)
        expected = hashlib.sha256(args.binary.read_bytes()).hexdigest()
        speech.wait_for_executable(args.container, expected)
        result = {'executable': expected, 'stages': []}
        try:
            for phase, present in enumerate([True, True, False, True, True]):
                (args.output / 'command').write_text(str(phase))
                receipt = args.output / f'phase-{phase}.json'
                deadline = time.monotonic() + 30
                while not receipt.exists():
                    if time.monotonic() > deadline:
                        raise TimeoutError(f'phase {phase} public update expired')
                    time.sleep(.05)
                row = json.loads(receipt.read_text())
                if row['phase'] != phase or row['track'] != args.track:
                    raise ValueError(f'preference changed CD selection: {row}')
                if result['stages'] and row['entity'] != result['stages'][0]['entity']:
                    raise ValueError('preference replaced restored playback entity')
                if row['paused'] != (not present):
                    raise ValueError('pause publication differs from preference')
                time.sleep(.2)
                captured = subprocess.run(['docker', 'exec', '-e', 'PULSE_SERVER=unix:/run/pulse/native',
                    args.container, 'timeout', '10', 'bash', '-c',
                    'parecord --raw --format=s16le --rate=22050 --channels=2 '
                    '--latency-msec=50 --device=cyrus.monitor | head -c 88200'],
                    capture_output=True, timeout=15, check=True)
                (args.output / f'phase-{phase}.pcm').write_bytes(captured.stdout)
                levels = speech.ambient.pcm_levels(captured.stdout)
                speech.ambient.assert_signal(levels, present)
                result['stages'].append({**row, 'rms': levels, 'expected_signal': present})
                (args.output / 'progress.json').write_text(json.dumps(result, indent=2)+'\n')
            (args.output / 'result.json').write_text(json.dumps(result, indent=2)+'\n')
            print(json.dumps(result, indent=2))
        finally:
            (args.output / 'command').write_text('5')


if __name__ == '__main__':
    main()
