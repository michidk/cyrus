#!/usr/bin/env python3
"""Admit one recording's isolated calibration, public cues and observed clocks.

Phase boundaries are recorded before analysis. The existing recorder pacing
allowance bounds PCM/wall epoch uncertainty; clock intervals retain that bound
and never interpolate host time into game time.
"""
import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path
import tempfile

import numpy as np
from scipy.io import wavfile


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def clock_bounds(samples, low, high):
    previous = None
    for value in samples:
        before, after, game = (value[k] for k in ('before', 'after', 'game_seconds'))
        if not all(math.isfinite(x) and x >= 0 for x in (before, after, game)) or before > after:
            raise ValueError('invalid observed clock sample')
        if previous and (before <= previous['after'] or game < previous['game_seconds']):
            raise ValueError('overlapping clock samples or game reset')
        previous = value
    left = [s for s in samples if s['after'] <= low]
    right = [s for s in samples if s['before'] >= high]
    if not left or not right:
        raise ValueError('PCM uncertainty interval is not bracketed by observed game clocks')
    return [left[-1]['game_seconds'], right[0]['game_seconds']]


def cadence_bounds(samples, first, second, allowance):
    """Bound game-clock cadence with one shared PCM epoch error for both cues."""
    if not first < second or allowance < 0:
        raise ValueError('ordered cue onsets and nonnegative uncertainty required')
    cuts = {-allowance, allowance}
    for sample in samples:
        for stamp in (sample['before'], sample['after']):
            for onset in (first, second):
                shift = stamp-onset
                if -allowance <= shift <= allowance:
                    cuts.add(shift)
    cuts = sorted(cuts)
    shifts = cuts + [(a+b)/2 for a,b in zip(cuts,cuts[1:])]
    bounds = []
    for shift in shifts:
        a = clock_bounds(samples, first+shift, first+shift)
        b = clock_bounds(samples, second+shift, second+shift)
        bounds.append([max(0, b[0]-a[1]), b[1]-a[0]])
    return [min(b[0] for b in bounds), max(b[1] for b in bounds)]


def analyze(root, references):
    capture = json.loads((root/'capture/result.json').read_text())
    phase = json.loads((root/'calibration-settings.json').read_text())
    control = json.loads((root/'calibration/result.json').read_text())
    if phase['before'] != phase['after'] or phase['before']['mute']:
        raise ValueError('calibration and public transition player settings differ')
    if not capture['success'] or not control['success']:
        raise ValueError('capture or calibration pacing failed')
    rate, pcm = wavfile.read(root/'capture/audio.wav')
    if rate != 48000 or pcm.dtype != np.int16 or pcm.ndim != 2 or pcm.shape[1] != 2:
        raise ValueError('paired recording requires signed16 stereo 48000 Hz')
    epoch = capture['epoch_monotonic_seconds']
    allowance = capture['pacing_tolerance_seconds']
    def segment(start, end):
        a = max(0, math.floor((start-allowance)*rate))
        b = min(len(pcm), math.ceil((end+allowance)*rate))
        if b <= a:
            raise ValueError('empty predetermined phase window')
        return a/rate, pcm[a:b]
    reference = root/'calibration/reference.wav'
    digest = hashlib.sha256(reference.read_bytes()).hexdigest()
    if digest != control['reference_sha256']:
        raise ValueError('calibration reference hash changed')
    _, signal = segment(phase['started']-epoch, phase['restored']-epoch)
    with tempfile.TemporaryDirectory() as temp:
        path = Path(temp)/'calibration.wav'; wavfile.write(path, rate, signal)
        unity = module('calibration', 'check-monitor-calibration.py').analyze(path, reference, control)
        event = capture['transitions'][0]
        # The driver owns acknowledgement and its tail. Clock observation must
        # not extend this window or pull in unrelated repeated book sounds.
        offset, transition = segment(event['sent'], len(pcm)/rate)
        path = Path(temp)/'transition.wav'; wavfile.write(path, rate, transition)
        continuity = module('continuity', 'book-audio-continuity.py')
        normalized = module('normalized', 'book-audio-normalize.py').report(
            continuity.load(path), [continuity.load(p) for p in references], event['sent']-offset)
    samples = [json.loads(line) for line in (root/'clocks.jsonl').read_text().splitlines()]
    input_bounds = clock_bounds(samples, epoch+event['sent']-allowance, epoch+event['sent']+allowance)
    for cue, path in zip(normalized['references'], references):
        cue.update(reference_sha256=hashlib.sha256(path.read_bytes()).hexdigest())
        if cue['success']:
            cue['pcm_onset_seconds'] = offset+cue['onset_seconds']
            bounds = clock_bounds(samples, epoch+cue['pcm_onset_seconds']-allowance,
                                  epoch+cue['pcm_onset_seconds']+allowance)
            cue['game_clock_bounds_seconds'] = bounds
            cue['input_relative_game_bounds_seconds'] = [bounds[0]-input_bounds[1], bounds[1]-input_bounds[0]]
    cadence = []
    cues = normalized['references']
    for i, first in enumerate(cues):
        for j, second in enumerate(cues):
            if first['success'] and second['success'] and first['pcm_onset_seconds'] < second['pcm_onset_seconds']:
                cadence.append(dict(first_reference=i, second_reference=j,
                    game_seconds=cadence_bounds(samples, epoch+first['pcm_onset_seconds'],
                        epoch+second['pcm_onset_seconds'], allowance)))
    return dict(success=bool(unity['success'] and normalized['success']), unity=unity,
                cadence=cadence,
                calibration_reference_sha256=digest, waveforms=normalized,
                input_game_clock_bounds_seconds=input_bounds,
                pcm_epoch_uncertainty_seconds=allowance,
                clock_samples=len(samples), monitor_sha256=hashlib.sha256((root/'capture/audio.wav').read_bytes()).hexdigest())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('recording', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--reference', action='append', required=True, type=Path)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('output exists; preserve evidence')
    result = analyze(args.recording, args.reference)
    args.output.write_text(json.dumps(result, indent=2)+'\n')
    raise SystemExit(0 if result['success'] else 1)


if __name__ == '__main__':
    main()
