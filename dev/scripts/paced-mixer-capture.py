#!/usr/bin/env python3
"""Record a real sink monitor around a bounded driver of acknowledged book transitions.

The driver receives CYRUS_CAPTURE_EPOCH (monotonic seconds) and prints JSON lines
with sent, acknowledged (seconds from that epoch), from and to page identities.
Only the driver knows host-specific readiness. Acknowledgement must observe the
requested destination after sending ordinary input, not echo the input command.
"""
import argparse
import array
import json
import math
import os
from pathlib import Path
import signal
import shlex
import subprocess
import sys
import time
import wave


def validate(path, wall_seconds, events, pacing_tolerance):
    with wave.open(str(path)) as source:
        if source.getparams()[:3] != (2, 2, 48000):
            raise ValueError('expected stereo signed 16-bit 48000 Hz monitor PCM')
        frames = source.getnframes()
        pcm = array.array('h', source.readframes(frames))
    if sys.byteorder != 'little':
        pcm.byteswap()
    seconds = frames / 48000
    if not frames or abs(seconds - wall_seconds) > pacing_tolerance:
        raise ValueError(f'monitor pacing mismatch: {seconds:.3f}s PCM / {wall_seconds:.3f}s wall')
    peak = max(map(abs, pcm), default=0)
    if not peak:
        raise ValueError('silent monitor recording')
    if not events:
        raise ValueError('no acknowledged transitions')
    for event in events:
        sent, ack = event.get('sent'), event.get('acknowledged')
        if (not isinstance(sent, (int, float)) or not isinstance(ack, (int, float))
                or not math.isfinite(sent) or not math.isfinite(ack)
                or not 0 <= sent < ack <= wall_seconds
                or event.get('from') is None or event.get('to') is None
                or event['from'] == event['to']):
            raise ValueError(f'invalid transition acknowledgement: {event}')
    envelope = [math.sqrt(sum(v*v for v in pcm[i:i+960]) / len(pcm[i:i+960]))
                for i in range(0, len(pcm), 960)]
    return dict(frames=frames, seconds=seconds, wall_seconds=wall_seconds,
                peak=peak, rms=math.sqrt(sum(v*v for v in pcm)/len(pcm)),
                envelope_10ms=envelope, transitions=events)


def require_ready(observation):
    if (not isinstance(observation, dict) or observation.get('ready') is not True
            or observation.get('page') is None):
        raise ValueError(f'book is not ready: {observation}')
    return observation


def capture(output, command, timeout, tolerance, ready_command):
    output.mkdir(parents=True, exist_ok=True)
    path = output / 'audio.wav'
    if path.exists():
        raise ValueError('audio.wav exists; preserve the run and choose another output')
    result = {'success': False, 'command': command}
    recorder = None
    try:
        readiness = subprocess.run(ready_command, capture_output=True, text=True,
                                   check=False, timeout=timeout)
        (output / 'readiness.json').write_text(readiness.stdout)
        (output / 'readiness.log').write_text(readiness.stderr)
        readiness.check_returncode()
        result['readiness'] = require_ready(json.loads(readiness.stdout))
        began = time.monotonic()
        with (output / 'recorder.log').open('w') as recorder_log:
            recorder = subprocess.Popen(['parec', '--device=cyrus.monitor', '--file-format=wav',
                '--format=s16le', '--rate=48000', '--channels=2', '--latency-msec=20', str(path)],
                stderr=recorder_log)
        deadline = began + 5
        while not path.exists() or path.stat().st_size <= 44:
            if recorder.poll() is not None or time.monotonic() >= deadline:
                raise RuntimeError('monitor did not admit PCM before input')
            time.sleep(.01)
        # Capture includes the recorder admission interval. Keep one clock for PCM and inputs.
        with (output / 'driver.jsonl').open('w') as log, (output / 'driver.log').open('w') as errors:
            subprocess.run(command, env={**os.environ, 'CYRUS_CAPTURE_EPOCH': str(began)},
                           stdout=log, stderr=errors,
                           check=True, timeout=timeout)
        ended = time.monotonic()
        recorder.send_signal(signal.SIGINT)
        code = recorder.wait(timeout=5)
        if code not in (0, -signal.SIGINT):
            raise RuntimeError(f'monitor recorder exited {code}')
        events = [json.loads(line) for line in (output / 'driver.jsonl').read_text().splitlines()]
        if not events or events[0].get('from') != result['readiness']['page']:
            raise ValueError('transition does not start at the observed ready page')
        result.update(validate(path, ended-began, events, tolerance), success=True)
    except Exception as error:
        result['error'] = str(error)
        raise
    finally:
        if recorder is not None and recorder.poll() is None:
            recorder.terminate()
            try:
                recorder.wait(timeout=5)
            except subprocess.TimeoutExpired:
                recorder.kill(); recorder.wait()
        (output / 'result.json').write_text(json.dumps(result, indent=2)+'\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ready-command', required=True, help='bounded observer command returning JSON ready and page before recording')
    parser.add_argument('output', type=Path)
    parser.add_argument('--timeout', type=float, default=45)
    parser.add_argument('--pacing-tolerance', type=float, default=.25,
                        help='maximum recorder admission/stop clock uncertainty in seconds')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.command[:1] == ['--']:
        args.command.pop(0)
    if not args.command or args.timeout <= 0 or args.pacing_tolerance <= 0:
        parser.error('require driver command and positive bounds')
    capture(args.output, args.command, args.timeout, args.pacing_tolerance, shlex.split(args.ready_command))


if __name__ == '__main__':
    main()
