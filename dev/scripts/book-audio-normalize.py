#!/usr/bin/env python3
"""Align admitted installed cue windows on a fractional-sample analysis grid.

Onsets use the recording's PCM epoch; --input-seconds supplies the acknowledged
input epoch in that same timeline. Gains describe the common 8 kHz analysis band,
not a calibrated device volume. Overlap annotations never remove another cue or
relax admission. References, recordings and reports remain local.
"""
import argparse
import hashlib
import importlib.util
import json
import math
from pathlib import Path

spec = importlib.util.spec_from_file_location('continuity', Path(__file__).with_name('book-audio-continuity.py'))
continuity = importlib.util.module_from_spec(spec)
spec.loader.exec_module(continuity)


def report(monitor, references, input_seconds):
    results = []
    for reference in references:
        value = continuity.normalize(monitor, reference)
        value['duration_seconds'] = len(reference) / continuity.RATE
        value['input_relative_onset_seconds'] = (
            value['onset_seconds'] - input_seconds if value['success'] else None)
        results.append(value)
    for index, value in enumerate(results):
        if not value['success']:
            continue
        for anchor in value['anchors']:
            start = anchor['monitor_seconds']
            anchor['overlaps'] = [other for other, cue in enumerate(results)
                                  if other != index and cue['success']
                                  and max(start, cue['onset_seconds']) <
                                  min(start + .1, cue['onset_seconds'] + cue['duration_seconds'])]
    return dict(success=all(cue['success'] for cue in results),
                input_seconds=input_seconds, references=results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('monitor', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--reference', action='append', required=True, type=Path)
    parser.add_argument('--input-seconds', required=True, type=float)
    args = parser.parse_args()
    if not math.isfinite(args.input_seconds) or args.input_seconds < 0:
        parser.error('input-seconds must be a finite nonnegative PCM timestamp')
    if args.output.exists():
        parser.error('output exists; preserve evidence and choose a new path')
    value = report(continuity.load(args.monitor),
                   [continuity.load(path) for path in args.reference], args.input_seconds)
    value.update(monitor=str(args.monitor),
                 monitor_sha256=hashlib.sha256(args.monitor.read_bytes()).hexdigest())
    for result, path in zip(value['references'], args.reference):
        result.update(reference=str(path), sha256=hashlib.sha256(path.read_bytes()).hexdigest())
    args.output.write_text(json.dumps(value, indent=2) + '\n')
    raise SystemExit(0 if value['success'] else 1)


if __name__ == '__main__':
    main()
