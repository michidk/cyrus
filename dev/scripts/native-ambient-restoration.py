#!/usr/bin/env python3
"""Observe live ambient gain binding after public session publication and dormant return.

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
    parser.add_argument('--runtime-output', required=True, help='output directory as mounted inside runtime')
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--emitter', required=True)
    parser.add_argument('--effect', type=int, required=True)
    parser.add_argument('--emitter-index', type=int, required=True)
    parser.add_argument('--resume-dormant', action='store_true', help='reuse five completed stages with matching executable and emitter')
    parser.add_argument('--scene', default='ISLAND')
    parser.add_argument('--destination', default='START')
    parser.add_argument('--seconds', type=float, default=180)
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
            capture.atomic(args.output / (name + '-complete.json'), json.dumps({
                'executable': actual, 'stage': name, 'observation': results['stages'][name]}, indent=2).encode())

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
            def place(dx):
                driver.request('place_player', x=point[0]+dx, y=point[1], z=point[2], heading=0)
                driver.request('reset_camera')
                driver.request('console', open=False)
                driver.progress(ready(args.scene))
                driver.request('console', open=True)

            if args.resume_dormant:
                retained = json.loads((args.output / 'progress.json').read_text())
                emitter = json.loads((args.output / 'emitter.json').read_text())
                required = {'before-publication', 'restored-continuation', 'restored-listener-moved',
                            'restored-stop', 'ordinary-poll-restarted-once'}
                if (retained['executable'] != actual or not required.issubset(retained['stages'])
                        or emitter['name'] != args.emitter or emitter['id']['index'] != args.emitter_index):
                    raise ValueError('resume receipt does not match executable, emitter or completed stages')
                for name in required:
                    stage = retained['stages'][name]
                    assert_signal(stage['rms'], stage['present'])
                slots = [slot for slot in capture.document((args.output / 'ambient.session').read_text())['samples']['ownership']['samples']['slots'] if slot]
                if len(slots) != 1 or slots[0]['effect'] != args.effect or slots[0]['owner'] != {'Script': emitter['id']}:
                    raise ValueError('resume sample owner differs from the selected fixture')
                results = retained
                pose = emitter['spatial']
                point = [v // 256 for v in ((pose.get('effective') or {}).get('position')
                    or pose.get('linked_position') or pose['local']['position'])]
            else:
                state = driver.request('state')
                emitter = None
                for offset in range(0, state['scripts']['instances'], 64):
                    rows = driver.request('scripts', offset=offset, limit=64)['actors']
                    emitter = next((row for row in rows if row['name'] == args.emitter and row['id']['index'] == args.emitter_index), None)
                    if emitter:
                        break
                if not emitter:
                    raise ValueError('selected installed emitter absent')
                capture.atomic(args.output / 'emitter.json', json.dumps(emitter, indent=2).encode())
                pose = emitter['spatial']
                point = [v // 256 for v in ((pose.get('effective') or {}).get('position')
                    or pose.get('linked_position') or pose['local']['position'])]

                place(512)
                record('before-publication', True)
                data = driver.capture()
                slots = [slot for slot in capture.document(data.decode())['samples']['ownership']['samples']['slots'] if slot]
                if len(slots) != 1 or slots[0]['effect'] != args.effect or slots[0]['owner'] != {'Script': emitter['id']}:
                    raise ValueError(f'PCM fixture selected a different live emitter: {slots}')
                capture.atomic(args.output / 'ambient.session', data)
                driver.request('session_load', path=str(Path(args.runtime_output) / 'ambient.session'))
                loaded = driver.wait(lambda: driver.terminal('session_load_result'), args.seconds)
                if loaded.get('committed') is not True:
                    raise ValueError(f'public session replacement failed: {loaded}')
                driver.request('session_load_result', acknowledge=True)
                driver.request('console', open=False)
                driver.progress(ready(args.scene))
                driver.request('console', open=True)
                record('restored-continuation', True)
                place(-512)
                record('restored-listener-moved', True)
                before = results['stages']['before-publication']['rms']
                after = results['stages']['restored-listener-moved']['rms']
                if abs(before[0]/before[1] - after[0]/after[1]) < .05:
                    raise ValueError('listener movement did not change restored stereo pan')
                driver.request('stop_sounds')
                record('restored-stop', False)
                driver.request('console', open=False)
                driver.progress(ready(args.scene))
                driver.request('console', open=True)
                record('ordinary-poll-restarted-once', True)
            driver.request('load_scene', name=args.destination)
            ready(args.destination)
            driver.request('console', open=True)
            driver.request('stop_sounds')
            record('dormant-owner-silent', False)
            driver.request('load_scene', name=args.scene)
            driver.request('console', open=False)
            driver.progress(ready(args.scene))
            place(0)
            record('dormant-restored-continuation', True)
            capture.atomic(args.output / 'result.json', json.dumps(results, indent=2).encode())
        finally:
            driver.request('console', open=False)


if __name__ == '__main__':
    main()
