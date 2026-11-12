#!/usr/bin/env python3
"""Check installed rustle continuity before comparing monitor onset or gain.

Requires numpy and scipy. References and monitor WAVs remain local. Independent
high-correlation windows must preserve their relative sample times; a paced sink
alone does not prove that the emulator supplied uninterrupted samples.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path

import numpy as np
from scipy.io import wavfile
from scipy.signal import correlate, fftconvolve, resample_poly

RATE = 8000


def load(path):
    rate, samples = wavfile.read(path)
    if samples.dtype != np.int16:
        raise ValueError(f'{path}: expected signed 16-bit PCM')
    samples = samples.astype(float)
    if samples.ndim == 2:
        samples = samples.mean(axis=1)
    if samples.ndim != 1 or not len(samples):
        raise ValueError(f'{path}: expected nonempty mono or stereo PCM')
    divisor = math.gcd(rate, RATE)
    return resample_poly(samples, RATE // divisor, rate // divisor)


def analyze(monitor, reference, minimum=.8, tolerance=.01, *, rate=RATE):
    """Find independent 100ms anchors and reject relative delay variation.

    Correlation admits anchors; it is not a fidelity score. The 10ms default
    drift limit is an analysis allowance, not an engine timing constant.
    """
    size = rate // 10
    anchors = []
    for offset in range(0, len(reference) - size + 1, size):
        block = reference[offset:offset + size]
        energy = np.dot(block, block)
        if energy == 0 or len(monitor) < size:
            continue
        dots = correlate(monitor, block, mode='valid', method='fft')
        # Local positive sums avoid catastrophic prefix subtraction in silent tails.
        if rate == RATE:
            local = np.convolve(monitor * monitor, np.ones(size), mode='valid')
        else:
            # Fine alignment has long windows. FFT sums retain the same
            # sub-unit-energy exclusion and clamp negative roundoff at silence.
            local = np.maximum(0, fftconvolve(monitor * monitor, np.ones(size), mode='valid'))
        # Sub-unit PCM energy cannot establish a waveform anchor. Resampler/FFT
        # roundoff in digital silence must not compete with an audible match.
        scores = np.divide(dots, np.sqrt(local * energy),
                           out=np.zeros_like(dots), where=local >= 1)
        index = int(np.argmax(scores))
        if scores[index] >= minimum:
            anchors.append(dict(reference_seconds=offset / rate,
                                monitor_seconds=index / rate,
                                correlation=float(scores[index]),
                                gain=float(dots[index] / energy)))
    enough = len(anchors) >= 3 and round((anchors[-1]['reference_seconds'] - anchors[0]['reference_seconds']) * rate) >= 3 * size
    delays = [a['monitor_seconds'] - a['reference_seconds'] for a in anchors]
    drift = max(delays) - min(delays) if delays else None
    continuous = enough and drift <= tolerance
    return dict(success=continuous, anchors=anchors, delay_spread_seconds=drift,
                minimum_correlation=minimum, allowed_drift_seconds=tolerance,
                error=None if continuous else ('insufficient independent reference anchors' if not enough else 'reference timing discontinuity'),
                # Only continuous evidence can support a shared onset/gain estimate.
                onset_seconds=float(np.median(delays)) if continuous else None,
                median_gain=float(np.median([a['gain'] for a in anchors])) if continuous else None)


def normalize(monitor, reference, subdivisions=16):
    """Refine fractional-sample alignment before estimating shared onset/gain.

    Both inputs already share the 8 kHz analysis band. Polyphase interpolation
    refines its 125 us grid without changing the correlation, independent-window
    span or drift requirements. It does not calibrate a device's frequency
    response, remove overlapping cues, or establish whole-waveform fidelity.
    """
    if subdivisions not in (2, 4, 8, 16, 32):
        raise ValueError('subdivisions must be 2, 4, 8, 16 or 32')
    result = analyze(resample_poly(monitor, subdivisions, 1),
                     resample_poly(reference, subdivisions, 1), rate=RATE * subdivisions)
    result['analysis_rate'] = RATE * subdivisions
    result['coarse'] = analyze(monitor, reference)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('monitor', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--reference', action='append', required=True, type=Path)
    parser.add_argument('--minimum-correlation', type=float, default=.8)
    parser.add_argument('--allowed-drift', type=float, default=.01)
    args = parser.parse_args()
    if not 0 < args.minimum_correlation <= 1 or args.allowed_drift <= 0:
        parser.error('require correlation in (0,1] and positive drift allowance')
    if args.output.exists():
        parser.error('output exists; preserve evidence and choose a new path')
    monitor = load(args.monitor)
    reports = []
    for path in args.reference:
        report = analyze(monitor, load(path), args.minimum_correlation, args.allowed_drift)
        report.update(reference=str(path), sha256=hashlib.sha256(path.read_bytes()).hexdigest())
        reports.append(report)
    result = dict(success=all(r['success'] for r in reports), monitor=str(args.monitor),
                  monitor_sha256=hashlib.sha256(args.monitor.read_bytes()).hexdigest(), references=reports)
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    raise SystemExit(0 if result['success'] else 1)


if __name__ == '__main__':
    main()
