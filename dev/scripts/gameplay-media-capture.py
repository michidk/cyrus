#!/usr/bin/env python3
"""Capture real PCM and clock-bracketed raster frames without stepping gameplay.

Local adapters observe running gameplay, trigger the prepared fixture, and save a
screenshot to their final argument. Observations contain ready, scene, frame,
game_seconds, integer elapsed_scale and normal_frame_rate, execution='running',
and player. Passive reads also retain elapsed_scale_end. They must never stop the game.
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
import wave
import time

spec = importlib.util.spec_from_file_location('mixer', Path(__file__).with_name('paced-mixer-capture.py'))
mixer = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mixer)


def require_observation(row):
    if (row.get('ready') is not True or not row.get('scene') or not row.get('player')
            or row.get('execution') != 'running'
            or type(row.get('frame')) is not int
            or type(row.get('game_seconds')) not in (int, float)
            or not math.isfinite(row['game_seconds'])):
        raise ValueError('require ready selected actor, finite clock and running execution')
    scale, rate = row.get('elapsed_scale'), row.get('normal_frame_rate')
    end = row.get('elapsed_scale_end', scale)
    if (type(scale) is not int or scale < 0 or type(end) is not int or end < scale
            or type(rate) is not int or rate <= 0
            or row['game_seconds'] != scale / (256 * rate)):
        raise ValueError('observation requires exact event-clock scale, read span and rate')
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
    accounting = boundary.get('clock_accounting', 'completed_updates')
    if accounting == 'includes_pending_update':
        pending = boundary.get('pending_frame_scale')
        accumulated = boundary.get('pending_accumulator_scale')
        if (type(pending) is not int or pending < 0 or type(accumulated) is not int
                or pending != accumulated or boundary.get('startup_remaining') != 0
                or scale < pending):
            raise ValueError('pending clock requires equal consumed/accumulated scale outside startup')
        # Clock publication precedes movement. Placement has not consumed this step.
        scale -= pending
        boundary = dict(boundary, raw_elapsed_scale=boundary['elapsed_scale'],
                        elapsed_scale=scale, game_seconds=scale / (256 * rate),
                        clock_accounting='completed_updates', excluded_pending_scale=pending)
    elif accounting != 'completed_updates':
        raise ValueError('unsupported placement clock accounting')
    elapsed_from_origin(boundary, observed)
    return boundary


def elapsed_from_origin(boundary, observed):
    require_observation(observed)
    if (observed['scene'] != boundary['scene'] or observed['frame'] < boundary['frame']
            or observed['game_seconds'] < boundary['game_seconds']
            or (boundary.get('generation') is not None and observed.get('generation') != boundary['generation'])):
        raise ValueError('scene or clock reset after fixture event')
    return observed['game_seconds'] - boundary['game_seconds']


def window(before, after, started, ended, pixels):
    require_observation(before)
    require_observation(after)
    if (before['scene'] != after['scene'] or after['frame'] < before['frame']
            or after['elapsed_scale'] < before.get('elapsed_scale_end', before['elapsed_scale'])
            or after['normal_frame_rate'] != before['normal_frame_rate']
            or (before.get('generation') is not None and after.get('generation') != before['generation'])
            or not 0 <= started <= ended):
        raise ValueError('scene or clock reset within raster window')
    return dict(before=before, after=after, wall_seconds=[started, ended],
                game_seconds=[before['game_seconds'], after['game_seconds']],
                pixels=pixels, atomic=False,
                clock_scale=[before['elapsed_scale'], after.get('elapsed_scale_end', after['elapsed_scale'])],
                normal_frame_rate=before['normal_frame_rate'])


def anchor_window(row, origin):
    if row['before']['scene'] != origin['scene'] or row['normal_frame_rate'] != origin['normal_frame_rate']:
        raise ValueError('window does not belong to the placement event clock')
    return dict(row, event_relative_scale=[value - origin['elapsed_scale'] for value in row['clock_scale']])


def owned_frame(response, path, delivered):
    """Use the producer's frozen frame, never the request or reply polling time."""
    if response.get('captured') is not True or response.get('path') != str(path):
        raise ValueError('owned frame requires a completed capture for the requested PNG')
    boundary = require_observation(response.get('boundary', {}))
    cursor = boundary.get('pcm_delivery_frames')
    if (boundary.get('event') != 'render_frame' or boundary.get('phase') not in ('render_extraction', 'sdl_surface_present')
            or boundary.get('elapsed_scale_end', boundary['elapsed_scale']) != boundary['elapsed_scale']
            or type(boundary.get('generation')) is not int
            or type(boundary.get('render_frame')) is not int or boundary['render_frame'] <= 0
            or type(cursor) is not int or not 0 <= cursor <= delivered):
        raise ValueError('owned frame requires a render-owned clock and PCM delivery cursor')
    return boundary


def pcm_frames(path):
    """Complete stereo s16 frames delivered to the recorder file, not mixer event time."""
    return path.stat().st_size // 4 if path.exists() else 0


def finalize_pcm(raw, wav):
    if raw.stat().st_size % 4:
        raise ValueError('monitor ended with an incomplete stereo PCM frame')
    with raw.open('rb') as source, wave.open(str(wav), 'wb') as target:
        target.setparams((2, 2, 48000, 0, 'NONE', 'not compressed'))
        while block := source.read(1024 * 1024):
            target.writeframesraw(block)


def summarize(rows, required_progress):
    if not rows:
        raise ValueError('no raster windows')
    for previous, current in zip(rows, rows[1:]):
        if (previous.get('clock_owner') in ('render_extraction', 'sdl_surface_present')
                and (current.get('clock_owner') != previous['clock_owner']
                     or current['before'].get('render_frame', 0) <= previous['after'].get('render_frame', 0))):
            raise ValueError('presentation sequence repeated or changed owner')
        window(previous['after'], current['before'], previous['wall_seconds'][1],
               current['wall_seconds'][0], current['pixels'])
    progress = rows[-1]['after']['game_seconds'] - rows[0]['before']['game_seconds']
    if progress < required_progress:
        raise ValueError('insufficient game-clock progress')
    return dict(game_seconds=progress, exact_update_coverage=False, raster_windows=len(rows))


def matching_intervals(left, right):
    """Admit equal event-relative raster and PCM windows, with no timing tolerance.

    This checks clock intervals only. It cannot make a non-atomic raster atomic
    or map PCM samples to an exact game event inside a wall-clock bracket.
    """
    def intervals(directory):
        result = json.loads((directory / 'result.json').read_text())
        if result.get('success') is not True:
            raise ValueError('capture did not complete')
        origin = trigger_origin(result['trigger'], result['trigger_observation'])
        rate = origin['normal_frame_rate']
        rows = [json.loads(line) for line in (directory / 'windows.jsonl').read_text().splitlines()]
        summarize(rows, 0)
        def clock_intervals(windows):
            times = []
            for row in windows:
                measured = window(row['before'], row['after'], *row['wall_seconds'], row['pixels'])
                anchored = anchor_window(measured, origin)
                times.append(tuple(Fraction(scale, 256 * rate) for scale in anchored['event_relative_scale']))
            return times
        pcm = [json.loads(line) for line in (directory / 'pcm-windows.jsonl').read_text().splitlines()]
        total = result['audio']['frames']
        if type(total) is not int or total <= 0:
            raise ValueError('PCM recording has no complete frames')
        previous_start = previous_end = 0
        for row in pcm:
            first, last = row['pcm_frames']
            if (type(first) is not int or type(last) is not int
                    or not previous_start <= first <= previous_end <= last <= total):
                raise ValueError('PCM delivery ranges have a gap, reset or invalid frame bound')
            previous_start, previous_end = first, last
        if not pcm or previous_end != total:
            raise ValueError('PCM clock windows do not cover the complete recording')
        return origin['scene'], clock_intervals(rows), clock_intervals(pcm)
    if intervals(left) != intervals(right):
        raise ValueError('capture event-relative game-clock intervals differ')
    return dict(matched_clock_intervals=True, matched_pcm_clock_intervals=True,
                atomic=False, exact_pcm_event_mapping=False)


def raster_metadata(path):
    data = path.read_bytes()
    if (len(data) < 33 or data[:8] != b'\x89PNG\r\n\x1a\n'
            or data[8:16] != b'\x00\x00\x00\rIHDR'):
        raise ValueError('screenshot adapter produced no PNG raster')
    width, height = struct.unpack('>II', data[16:24])
    if not width or not height:
        raise ValueError('screenshot adapter produced an empty raster')
    return dict(width=width, height=height, sha256=hashlib.sha256(data).hexdigest())


def capture(output, observe, trigger, screenshot, seconds, timeout, interval, owned_frames=False):
    # Exclusive evidence directory prevents a retry from overwriting admitted media.
    output.mkdir(parents=True, exist_ok=False)
    result = dict(success=False, exact_update_coverage=False)
    recorder = None
    rows = []
    points = []
    origin = None
    raw = output / 'audio.raw'
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

    def retain(observed, started, ended, cursors):
        point = dict(observation=observed, wall_seconds=[started, ended],
                     epoch_monotonic_seconds=epoch, pcm_frames=cursors)
        points.append(point)
        with (output / 'observations.jsonl').open('a') as log:
            log.write(json.dumps(point) + '\n')
        persist_pcm_windows()

    def sample():
        started, first = time.monotonic() - epoch, pcm_frames(raw)
        observed = require_observation(json.loads(run(observe)))
        retain(observed, started, time.monotonic() - epoch, [first, pcm_frames(raw)])
        return observed

    def persist_windows():
        with (output / 'windows.jsonl').open('w') as log:
            for row in rows:
                log.write(json.dumps(anchor_window(row, origin) if origin else row) + '\n')

    def persist_pcm_windows():
        with (output / 'pcm-windows.jsonl').open('w') as log:
            for before, after in zip(points, points[1:]):
                row = window(before['observation'], after['observation'],
                             before['wall_seconds'][0], after['wall_seconds'][1], None)
                row.update(pcm_frames=[before['pcm_frames'][0], after['pcm_frames'][1]],
                           pcm_mapping='recorder_delivery_bracket')
                log.write(json.dumps(anchor_window(row, origin) if origin else row) + '\n')

    try:
        baseline = sample()
        result['baseline'] = baseline
        epoch = time.monotonic()
        result['epoch_monotonic_seconds'] = epoch
        points[:] = [dict(observation=baseline, wall_seconds=[0, 0], pcm_frames=[0, 0])]
        path = output / 'audio.wav'
        with (output / 'recorder.log').open('w') as log:
            recorder = subprocess.Popen(['parec', '--device=cyrus.monitor', '--raw',
                '--format=s16le', '--rate=48000', '--channels=2', '--latency-msec=20', str(raw)], stderr=log)
        while pcm_frames(raw) == 0:
            if recorder.poll() is not None or time.monotonic() > min(deadline, epoch + 5):
                raise RuntimeError('monitor did not admit PCM before trigger')
            time.sleep(.01)
        # Retain a real raster before trigger, with the recorder already admitted.
        def raster():
            started, first = time.monotonic() - epoch, pcm_frames(raw)
            pixels = f'frame-{len(rows):04}.png'
            path = output / pixels
            if owned_frames:
                response = json.loads(run(screenshot + [str(path)]))
                before = after = owned_frame(response, path, pcm_frames(raw))
                first = last = before['pcm_delivery_frames']
                retain(before, started, time.monotonic() - epoch, [first, last])
            else:
                before = sample()
                run(screenshot + [str(path)])
                after = sample()
                last = pcm_frames(raw)
            ended = time.monotonic() - epoch
            metadata = raster_metadata(path)
            row = window(before, after, started, ended, pixels)
            row['raster'] = metadata
            row['pcm_frames'] = [first, last]
            row['clock_owner'] = before['phase'] if owned_frames else 'observation_bracket'
            row['pcm_mapping'] = 'recorder_delivery_cursor' if owned_frames else 'recorder_delivery_bracket'
            rows.append(row)
            persist_windows()
        raster()
        sent = time.monotonic() - epoch
        result['trigger'] = json.loads(run(trigger))
        acknowledged = time.monotonic() - epoch
        if owned_frames:
            raster()
            result['trigger_observation'] = rows[-1]['after']
        else:
            result['trigger_observation'] = sample()
        trigger_boundary = trigger_origin(result['trigger'], result['trigger_observation'])
        result['trigger_boundary'] = trigger_boundary
        origin = trigger_boundary
        persist_windows()
        persist_pcm_windows()
        if not owned_frames:
            raster()
        while True:
            post_trigger_progress = elapsed_from_origin(trigger_boundary, rows[-1]['after'])
            if post_trigger_progress >= seconds:
                break
            time.sleep(min(interval, max(0, deadline - time.monotonic())))
            raster()
        ended = time.monotonic()
        recorder.send_signal(signal.SIGINT)
        code = recorder.wait(timeout=5)
        if code not in (0, -signal.SIGINT):
            raise RuntimeError(f'monitor recorder exited {code}')
        # Bound the recorder tail with a running observation, without treating file
        # delivery as a zero-latency mixer event or interpolating game time.
        sample()
        finalize_pcm(raw, path)
        # Reuse the real monitor pacing/non-silence gate, with explicit fixture epochs.
        audio = mixer.validate(path, ended - epoch,
            [dict(sent=sent, acknowledged=acknowledged, **{'from': 'prepared', 'to': 'triggered'})], .25)
        audio.update(exact_pcm_event_mapping=False, clock_windows='pcm-windows.jsonl')
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
        result['pcm_delivery_frames'] = pcm_frames(raw)
        result['pcm_unobserved_tail_frames'] = pcm_frames(raw) - (points[-1]['pcm_frames'][1] if points else 0)
        (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path, nargs='?')
    parser.add_argument('--compare-captures', type=Path, nargs=2)
    parser.add_argument('--owned-frames', action='store_true', help='screenshot adapter returns capture_gameplay_frame result with a frozen render boundary')
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
            shlex.split(args.screenshot), args.game_seconds, args.timeout, args.interval, args.owned_frames)


if __name__ == '__main__':
    main()
