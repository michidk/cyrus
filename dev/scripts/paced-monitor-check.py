#!/usr/bin/env python3
"""Bounded PulseAudio control independent of an emulator or book input.

Run inside the paced runtime. Retain the generated reference, monitor WAV and
clock report, then use book-audio-continuity.py on the host to verify sample time.
An explicit installed reference can replace the generated deterministic noise.
"""
import argparse
import array
import json
import hashlib
from pathlib import Path
import random
import signal
import subprocess
import sys
import time
import wave


def reference(path):
    rng = random.Random(62)
    samples = array.array('h')
    for _ in range(3 * 48000):
        value = rng.randrange(-4000, 4001)
        samples.extend((value, value))
    if sys.byteorder != 'little':
        samples.byteswap()
    with wave.open(str(path), 'wb') as target:
        target.setparams((2, 2, 48000, 0, 'NONE', 'not compressed'))
        target.writeframes(samples.tobytes())


def stop(process):
    if process is not None and process.poll() is None:
        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def run(output, source, latency, warm=True, playback_volume=65536):
    output.mkdir(parents=True, exist_ok=True)
    path = output / 'monitor.wav'
    if path.exists() or (output / 'result.json').exists():
        raise ValueError('evidence exists; choose a fresh output')
    if source is None:
        source = output / 'reference.wav'
        reference(source)
    result = {'success': False, 'reference': str(source),
              'reference_sha256': hashlib.sha256(source.read_bytes()).hexdigest(),
              'recorder_latency_ms': latency, 'warm_sink': warm,
              'playback_volume': playback_volume}
    if not 0 < playback_volume <= 65536:
        raise ValueError('playback volume must be in 1..65536')
    keepalive = recorder = player = None
    try:
        if warm:
            # A monitor alone does not lower the null sink's idle two-second clock.
            keepalive = subprocess.Popen(['pacat', '--playback', '--raw', '--format=s16le',
                '--rate=48000', '--channels=2', '--latency-msec=20', '/dev/zero'],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            # The idle null sink may already have a two-second timer scheduled.
            time.sleep(2.1)
        began = time.monotonic()
        with (output / 'recorder.log').open('w') as errors:
            recorder = subprocess.Popen(['parec', '--device=cyrus.monitor', '--file-format=wav',
                '--format=s16le', '--rate=48000', '--channels=2',
                *([f'--latency-msec={latency}'] if latency else []), str(path)], stderr=errors)
        deadline = began + 5
        while not path.exists() or path.stat().st_size <= 44:
            if recorder.poll() is not None or time.monotonic() >= deadline:
                raise RuntimeError('control recorder did not admit PCM')
            time.sleep(.01)
        result['admission_seconds'] = time.monotonic() - began
        time.sleep(.2)
        sent = time.monotonic()
        player = subprocess.Popen(['paplay', '--latency-msec=20',
            f'--volume={playback_volume}', '--client-name=cyrus-gain-calibration', str(source)])
        result['playback_pid'] = player.pid
        deadline = sent + 5
        while True:
            snapshot = subprocess.run(['pactl', 'list', 'sink-inputs'],
                                      capture_output=True, text=True, check=True, timeout=5)
            if f'application.process.id = "{player.pid}"' in snapshot.stdout:
                (output / 'sink-inputs.log').write_text(snapshot.stdout)
                break
            if player.poll() is not None or time.monotonic() >= deadline:
                raise RuntimeError('calibration playback stream not observed while active')
            time.sleep(.01)
        for kind in ('sinks', 'sources', 'source-outputs'):
            snapshot = subprocess.run(['pactl', 'list', kind], capture_output=True,
                                      text=True, check=True, timeout=5)
            (output / (kind + '.log')).write_text(snapshot.stdout + snapshot.stderr)
        player.wait(timeout=10)
        if player.returncode:
            raise RuntimeError(f'calibration playback failed: {player.returncode}')
        result['play_seconds'] = time.monotonic() - sent
        time.sleep(.2)
        ended = time.monotonic()
        stop(recorder)
        with wave.open(str(path)) as recording:
            seconds = recording.getnframes() / recording.getframerate()
        result.update(wall_seconds=ended-began, pcm_seconds=seconds)
        if abs(seconds - (ended-began)) > .25:
            raise ValueError('control monitor pacing mismatch')
        result['success'] = True
    except Exception as error:
        result['error'] = str(error)
        raise
    finally:
        stop(player)
        stop(recorder)
        stop(keepalive)
        (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--reference', type=Path)
    parser.add_argument('--recorder-latency-ms', type=int, default=20,
                        help='use 0 for the server-default delayed-drain control')
    parser.add_argument('--playback-volume', type=int, default=65536,
                        help='explicit PulseAudio stream volume; unity is 65536')
    parser.add_argument('--cold-sink', action='store_true', help='omit warm playback for the idle-clock failure control')
    args = parser.parse_args()
    if args.recorder_latency_ms < 0:
        parser.error('latency must be nonnegative')
    run(args.output, args.reference, args.recorder_latency_ms, not args.cold_sink, args.playback_volume)


if __name__ == '__main__':
    main()
