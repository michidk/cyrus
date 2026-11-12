#!/usr/bin/env python3
"""Require an unscaled, same-rate PCM calibration before using monitor gain.

This is a unity capture-chain control, not a production gain tolerance. Different
sample rates require a separately calibrated resampler and are rejected here.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import numpy as np
from scipy.io import wavfile

spec = importlib.util.spec_from_file_location('continuity', Path(__file__).with_name('book-audio-continuity.py'))
continuity = importlib.util.module_from_spec(spec)
spec.loader.exec_module(continuity)


def analyze(monitor, reference, capture):
    rate, recorded = wavfile.read(monitor)
    source_rate, source = wavfile.read(reference)
    if rate != source_rate or recorded.dtype != np.int16 or source.dtype != np.int16:
        raise ValueError('unity control requires same-rate signed16 PCM')
    if recorded.ndim != source.ndim or recorded.shape[1:] != source.shape[1:]:
        raise ValueError('unity control channel layouts differ')
    recorded = recorded.astype(float)
    source = source.astype(float)
    if recorded.ndim == 1:
        recorded, source = recorded[:, None], source[:, None]
    channels = []
    for index in range(source.shape[1]):
        result = continuity.analyze(recorded[:, index], source[:, index], rate=rate)
        # Only FFT roundoff is allowed. This does not admit a different mixer gain.
        unity = result['success'] and abs(result['median_gain'] - 1) <= 64 * np.finfo(float).eps
        result.update(unity=bool(unity), channel=index)
        channels.append(result)
    return dict(success=bool(capture['success'] and all(channel['unity'] for channel in channels)),
                pacing=bool(capture['success']), rate=rate, channels=channels)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture', type=Path, help='paced-monitor-check evidence directory')
    parser.add_argument('reference', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    if args.output.exists():
        parser.error('output exists; preserve evidence')
    capture = json.loads((args.capture / 'result.json').read_text())
    digest = hashlib.sha256(args.reference.read_bytes()).hexdigest()
    if digest != capture['reference_sha256']:
        parser.error('reference differs from the captured calibration hash')
    result = analyze(args.capture / 'monitor.wav', args.reference, capture)
    result.update(reference_sha256=digest,
                  monitor_sha256=hashlib.sha256((args.capture / 'monitor.wav').read_bytes()).hexdigest())
    args.output.write_text(json.dumps(result, indent=2) + '\n')
    raise SystemExit(0 if result['success'] else 1)


if __name__ == '__main__':
    main()
