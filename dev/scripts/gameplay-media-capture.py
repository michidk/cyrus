#!/usr/bin/env python3
"""Capture real PCM and clock-bracketed raster frames without stepping gameplay.

Local adapters observe running gameplay, trigger the prepared fixture, and save a
screenshot to their final argument. Observations contain ready, scene, frame,
game_seconds, execution='running', and player. They must never stop the game.
Trigger adapters return acknowledged=true plus a boundary with ready, scene, frame,
player, game_seconds, elapsed_scale (integer Q8 nominal ticks), normal_frame_rate,
execution, event='placement_applied', and phase='before_ordinary_update'. The
boundary must be captured when placement is applied, not by a subsequent poll.
These windows do not establish atomic pose/pixel correspondence or exact parity.
"""
import argparse
import importlib.util
import hashlib
from fractions import Fraction
import json
import math
from pathlib import Path
import shlex
import signal
import subprocess
import struct
import time

spec = importlib.util.spec_from_file_location('mixer', Path(__file__).with_name('paced-mixer-capture.py'))
mixer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mixer)


def require_observation(row):
    if (row.get('ready') is not True or not row.get('scene') or not row.get('player')
            or row.get('execution') != 'running'
            or type(row.get('frame')) is not int
            or not isinstance(row.get('game_seconds'), (int, float))
            or not math.isfinite(row['game_seconds'])):
        raise ValueError('require ready selected actor, finite clock and running execution')
    return row


def trigger_origin(acknowledgement, observed):
    """Admit placement event time, never substitute a later polling clock.

    The setup event may stop execution briefly. Raster observations must still
    come from running execution. Integer scale is in Q8 nominal ticks.
    """
    boundary = acknowledgement.get('boundary')
    if acknowledgement.get('acknowledged') is not True or not isinstance(boundary, dict):
        raise ValueError('fixture trigger requires an acknowledged event boundary')
    scale, rate = boundary.get('elapsed_scale'), boundary.get('normal_frame_rate')
    if (type(scale) is not int or scale < 0 or type(rate) is not int or rate <= 0
            or boundary.get('event') != 'placement_applied'
            or boundary.get('phase') != 'before_ordinary_update'
            or boundary.get('execution') not in ('running', 'stopped')):
        raise ValueError('fixture trigger requires an exact placement event clock and phase')
    require_observation({**boundary, 'execution': 'running'})
    if boundary['game_seconds'] != scale / (256 * rate):
        raise ValueError('fixture event clock disagrees with integer elapsed scale')
    elapsed_from_origin(boundary, observed)
    return boundary


def elapsed_from_origin(boundary, observed):
    require_observation(observed)
    if (observed['scene'] != boundary['scene'] or observed['frame'] < boundary['frame']
            or observed['game_seconds'] < boundary['game_seconds']):
        raise ValueError('scene or clock reset after fixture event')
    return observed['game_seconds'] - boundary['game_seconds']


def window(before, after, started, ended, pixels):
    require_observation(before)
    require_observation(after)
    if (before['scene'] != after['scene'] or after['frame'] < before['frame']
            or after['game_seconds'] < before['game_seconds']
            or not 0 <= started <= ended):
        raise ValueError('scene or clock reset within raster window')
    return dict(before=before, after=after, wall_seconds=[started, ended],
                game_seconds=[before['game_seconds'], after['game_seconds']],
                pixels=pixels, atomic=False)


def summarize(rows, required_progress):
    if not rows:
        raise ValueError('no raster windows')
    for previous, current in zip(rows, rows[1:]):
        window(previous['after'], current['before'], previous['wall_seconds'][1],
               current['wall_seconds'][0], current['pixels'])
    progress = rows[-1]['after']['game_seconds'] - rows[0]['before']['game_seconds']
    if progress < required_progress:
        raise ValueError('insufficient game-clock progress')
    return dict(game_seconds=progress, exact_update_coverage=False, raster_windows=len(rows))


def matching_intervals(left, right):
    """Admit equal event-relative raster windows, with no timing tolerance.

    This checks clock intervals only. It cannot make a non-atomic raster atomic
    or map PCM samples to an exact game event inside a wall-clock bracket.
    """
    def intervals(directory):
        result = json.loads((directory / 'result.json').read_text())
        if result.get('success') is not True:
            raise ValueError('capture did not complete')
        origin = trigger_origin(result['trigger'], result['trigger_observation'])
        rate = origin['normal_frame_rate']
        start = origin['elapsed_scale']
        rows = [json.loads(line) for line in (directory / 'windows.jsonl').read_text().splitlines()]
        summarize(rows, 0)
        times = []
        for row in rows:
            window(row['before'], row['after'], *row['wall_seconds'], row['pixels'])
            bounds = []
            for observation in (row['before'], row['after']):
                scale = observation.get('elapsed_scale')
                if (observation['scene'] != origin['scene'] or type(scale) is not int or scale < 0
                        or observation.get('normal_frame_rate') != rate
                        or observation['game_seconds'] != scale / (256 * rate)):
                    raise ValueError('raster bounds require exact event-clock scale and rate')
                bounds.append(Fraction(scale - start, 256 * rate))
            times.append(tuple(bounds))
        return origin['scene'], times
    if intervals(left) != intervals(right):
        raise ValueError('capture event-relative game-clock intervals differ')
    return dict(matched_clock_intervals=True, atomic=False, exact_pcm_event_mapping=False)


def raster_metadata(path):
    data = path.read_bytes()
    if (len(data) < 33 or data[:8] != b'\x89PNG\r\n\x1a\n'
            or data[8:16] != b'\x00\x00\x00\rIHDR'):
        raise ValueError('screenshot adapter produced no PNG raster')
    width, height = struct.unpack('>II', data[16:24])
    if not width or not height:
        raise ValueError('screenshot adapter produced an empty raster')
    return dict(width=width, height=height, sha256=hashlib.sha256(data).hexdigest())


def capture(output, observe, trigger, screenshot, seconds, timeout, interval):
    # Exclusive evidence directory prevents a retry from overwriting admitted media.
    output.mkdir(parents=True, exist_ok=False)
    result = dict(success=False, exact_update_coverage=False)
    recorder = None
    rows = []
    epoch = time.monotonic()
    deadline = epoch + timeout

    def run(command):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError('gameplay media game-clock deadline')
        proc = subprocess.run(command, capture_output=True, text=True, timeout=remaining)
        with (output / 'adapters.log').open('a') as log:
            log.write(json.dumps(dict(command=command, code=proc.returncode,
                                      stdout=proc.stdout, stderr=proc.stderr)) + '\n')
        proc.check_returncode()
        return proc.stdout

    def sample():
        return require_observation(json.loads(run(observe)))

    try:
        baseline = sample()
        result['baseline'] = baseline
        epoch = time.monotonic()
        result['epoch_monotonic_seconds'] = epoch
        path = output / 'audio.wav'
        with (output / 'recorder.log').open('w') as log:
            recorder = subprocess.Popen(['parec', '--device=cyrus.monitor', '--file-format=wav',
                '--format=s16le', '--rate=48000', '--channels=2', '--latency-msec=20', str(path)], stderr=log)
        while not path.exists() or path.stat().st_size <= 44:
            if recorder.poll() is not None or time.monotonic() > min(deadline, epoch + 5):
                raise RuntimeError('monitor did not admit PCM before trigger')
            time.sleep(.01)
        # Retain a real raster before trigger, with the recorder already admitted.
        def raster():
            started = time.monotonic() - epoch
            before = sample()
            pixels = f'frame-{len(rows):04}.png'
            run(screenshot + [str(output / pixels)])
            after = sample()
            ended = time.monotonic() - epoch
            metadata = raster_metadata(output / pixels)
            row = window(before, after, started, ended, pixels)
            row['raster'] = metadata
            rows.append(row)
            with (output / 'windows.jsonl').open('a') as log:
                log.write(json.dumps(row) + '\n')
        raster()
        sent = time.monotonic() - epoch
        result['trigger'] = json.loads(run(trigger))
        acknowledged = time.monotonic() - epoch
        result['trigger_observation'] = sample()
        trigger_boundary = trigger_origin(result['trigger'], result['trigger_observation'])
        result['trigger_boundary'] = trigger_boundary
        while True:
            raster()
            post_trigger_progress = elapsed_from_origin(trigger_boundary, rows[-1]['after'])
            if post_trigger_progress >= seconds:
                break
            time.sleep(min(interval, max(0, deadline - time.monotonic())))
        ended = time.monotonic()
        recorder.send_signal(signal.SIGINT)
        code = recorder.wait(timeout=5)
        if code not in (0, -signal.SIGINT):
            raise RuntimeError(f'monitor recorder exited {code}')
        # Reuse the real monitor pacing/non-silence gate, with explicit fixture epochs.
        audio = mixer.validate(path, ended - epoch,
            [dict(sent=sent, acknowledged=acknowledged, **{'from': 'prepared', 'to': 'triggered'})], .25)
        result.update(summarize(rows, seconds), audio=audio,
                      post_trigger_game_seconds=post_trigger_progress, success=True)
    except Exception as error:
        result['error'] = str(error)
        raise
    finally:
        if recorder is not None and recorder.poll() is None:
            recorder.terminate()
            try:
                recorder.wait(timeout=5)
            except subprocess.TimeoutExpired:
                recorder.kill()
                recorder.wait()
        (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, nargs='?')
    parser.add_argument('--compare-captures', type=Path, nargs=2)
    parser.add_argument('--observe')
    parser.add_argument('--trigger')
    parser.add_argument('--screenshot')
    parser.add_argument('--game-seconds', type=float, default=3)
    parser.add_argument('--timeout', type=float, default=90)
    parser.add_argument('--interval', type=float, default=.1)
    args = parser.parse_args()
    if args.compare_captures:
        print(json.dumps(matching_intervals(*args.compare_captures)))
        return
    if not all((args.output, args.observe, args.trigger, args.screenshot)):
        parser.error('capture requires output, --observe, --trigger and --screenshot')
    if any(not math.isfinite(v) or v <= 0 for v in (args.game_seconds, args.timeout, args.interval)):
        parser.error('capture bounds must be finite and positive')
    capture(args.output, shlex.split(args.observe), shlex.split(args.trigger),
            shlex.split(args.screenshot), args.game_seconds, args.timeout, args.interval)


if __name__ == '__main__':
    main()
