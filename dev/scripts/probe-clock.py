#!/usr/bin/env python3
"""Plan exact shared acquisition intervals from acknowledged simulation boundaries.

A steady tail is a measurement used to propose a schedule, not a prediction that
FPS will stay fixed. Validate the complete executed schedule before comparing
outcomes. Waiting for a measurement never changes the simulation clock or cadence.
"""
import argparse
import json
import math
from pathlib import Path


def intervals(boundaries):
    """Require a single contiguous ordinary-update lifetime with exact integer clocks."""
    if len(boundaries) < 2:
        raise ValueError('ordinary clock measurement needs at least two boundaries')
    for row in boundaries:
        if any(type(row.get(key)) is not int or row[key] < 0
               for key in ('update', 'generation', 'consumed_scale')):
            raise ValueError('ordinary boundary clock must be nonnegative integers')
    result = []
    for before, after in zip(boundaries, boundaries[1:]):
        if after['generation'] != before['generation'] or after['update'] != before['update'] + 1:
            raise ValueError('ordinary clock boundary skipped, repeated or restarted')
        step = after['consumed_scale'] - before['consumed_scale']
        if step <= 0:
            raise ValueError('ordinary consumed scale did not advance')
        result.append(step)
    return result


def steady_step(boundaries, tail=8):
    """Measure a contiguous steady tail; tail length is an acquisition policy, not an engine rule."""
    if type(tail) is not int or tail < 2:
        raise ValueError('steady observation needs at least two intervals')
    steps = intervals(boundaries)
    if len(steps) < tail or len(set(steps[-tail:])) != 1:
        raise ValueError('ordinary clock has no measured steady tail')
    return steps[-1]


def plan(original, native, *, normal_frame_rate, minimum_scale, max_updates=255):
    """Choose a shared integer endpoint; rate comes from installed timing/receipt metadata."""
    if any(type(value) is not int or value <= 0
           for value in (normal_frame_rate, minimum_scale, max_updates)):
        raise ValueError('clock rate, minimum scale and update bound must be positive integers')
    a, b = steady_step(original), steady_step(native)
    quantum = math.lcm(a, b)
    target = ((minimum_scale + quantum - 1) // quantum) * quantum
    if max(target // a, target // b) > max_updates:
        raise ValueError('common clock endpoint exceeds bounded acquisition')
    return dict(target_scale=target, normal_frame_rate=normal_frame_rate,
                original_updates=target // a, native_updates=target // b,
                original_step=a, native_step=b)


def validate(boundaries, expected, side):
    """Reject actual cadence changes, missing boundaries and skipped endpoints without interpolation."""
    if side not in ('original', 'native'):
        raise ValueError('clock side must be original or native')
    steps = intervals(boundaries)
    if (len(steps) != expected[side + '_updates']
            or any(step != expected[side + '_step'] for step in steps)
            or sum(steps) != expected['target_scale']):
        raise ValueError('executed clock boundaries differ from the exact acquisition plan')


def pair_observed(original, native, *, original_rate, native_rate, required_scale=None):
    """Pair only real, contiguous event-relative receipts; zero is the acknowledged input origin.

    An earlier common boundary establishes only that measured interval. It never
    substitutes for a requested later endpoint or establishes gameplay pose parity.
    """
    if (type(original_rate) is not int or original_rate <= 0
            or type(native_rate) is not int or native_rate != original_rate):
        raise ValueError('paired clock rates must be equal positive installed rates')
    if required_scale is not None and (type(required_scale) is not int or required_scale <= 0):
        raise ValueError('required endpoint must be a positive integer scale')
    intervals(original)
    intervals(native)
    if original[0]['consumed_scale'] != 0 or native[0]['consumed_scale'] != 0:
        raise ValueError('paired input origins must both have zero consumed scale')
    by_scale = {row['consumed_scale']: i for i, row in enumerate(native)}
    paired = [dict(consumed_scale=row['consumed_scale'], original_index=i,
                   native_index=by_scale[row['consumed_scale']])
              for i, row in enumerate(original)
              if row['consumed_scale'] > 0 and row['consumed_scale'] in by_scale]
    if required_scale is not None:
        paired = [pair for pair in paired if pair['consumed_scale'] == required_scale]
    if not paired:
        raise ValueError('shared requested endpoint not observed in both acquisitions')
    return paired


class Admission:
    """Select a counterpart endpoint online at an actual input-owning boundary.

    The counterpart must retain contiguous receipts from its input origin. Call
    observe before sampling input, release and snapshot only when it returns
    True. No steady-step estimate is used. Exhaustion rejects the acquisition.
    """

    def __init__(self, counterpart, *, minimum_scale, max_updates):
        intervals(counterpart)
        if counterpart[0]['consumed_scale'] != 0:
            raise ValueError('counterpart input origin must have zero consumed scale')
        if any(type(value) is not int or value <= 0 for value in (minimum_scale, max_updates)):
            raise ValueError('minimum scale and update bound must be positive integers')
        self.candidates = {row['consumed_scale']: index for index, row in enumerate(counterpart)
                           if row['consumed_scale'] >= minimum_scale}
        if not self.candidates:
            raise ValueError('no observed counterpart endpoint at requested duration')
        self.max_updates = max_updates
        self.previous = None
        self.count = 0
        self.endpoint = None
        self.rejected = False
        self.counterpart_index = None

    def observe(self, boundary):
        if self.rejected:
            raise ValueError('acquisition already rejected')
        try:
            return self._observe(boundary)
        except ValueError:
            self.rejected = True
            raise

    def _observe(self, boundary):
        if self.endpoint is not None:
            raise ValueError('acquisition already completed')
        if self.previous is None:
            # Validate integer fields even before there is a second boundary.
            if any(type(boundary.get(key)) is not int or boundary[key] < 0
                   for key in ('generation', 'update', 'consumed_scale')):
                raise ValueError('ordinary boundary clock must be nonnegative integers')
            if boundary['consumed_scale'] != 0:
                raise ValueError('input origin must have zero consumed scale')
        else:
            intervals([self.previous, boundary])
            self.count += 1
        scale = boundary['consumed_scale']
        self.previous = dict(boundary)
        if scale in self.candidates:
            self.endpoint = scale
            self.counterpart_index = self.candidates[scale]
            return True
        if scale > max(self.candidates) or self.count >= self.max_updates:
            raise ValueError('observed counterpart endpoints unavailable within acquisition bound')
        return False


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('original', type=Path, help='normalized acknowledged boundary JSON array')
    parser.add_argument('native', type=Path, help='normalized acknowledged boundary JSON array')
    parser.add_argument('--normal-frame-rate', type=int, required=True)
    parser.add_argument('--minimum-scale', type=int, required=True)
    args = parser.parse_args()
    print(json.dumps(plan(json.loads(args.original.read_text()), json.loads(args.native.read_text()),
                          normal_frame_rate=args.normal_frame_rate, minimum_scale=args.minimum_scale), indent=2))


if __name__ == '__main__':
    main()
