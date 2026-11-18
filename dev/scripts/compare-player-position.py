#!/usr/bin/env python3
"""Compare Q8 positions at equal elapsed clock units using linear interpolation.

Both inputs are JSON arrays of {elapsed_scale: integer, position_q8: [x, y, z]}.
Callers establish matching clock units, origin, scene and position coordinate space.
This compares positions only: it never interpolates animation or controller state,
extrapolates beyond reference coverage, or establishes equivalent starting state.
Exit 0 means all native observations match; 1 means differences or missing coverage;
2 means malformed input. Fractional results are reported as JSON numbers.
"""
import argparse
from bisect import bisect_left
from fractions import Fraction
import json
from pathlib import Path


def read_series(path, name):
    rows = json.loads(Path(path).read_text())
    if not isinstance(rows, list) or not rows:
        raise ValueError(f'{name}: expected a nonempty array')
    previous = None
    for index, row in enumerate(rows):
        if not isinstance(row, dict) or type(row.get('elapsed_scale')) is not int:
            raise ValueError(f'{name} row {index}: elapsed_scale must be an integer')
        clock = row['elapsed_scale']
        if clock < 0 or (previous is not None and clock <= previous):
            raise ValueError(f'{name} row {index}: elapsed_scale must increase strictly from zero or above')
        position = row.get('position_q8')
        if not isinstance(position, list) or len(position) != 3 or any(type(v) is not int for v in position):
            raise ValueError(f'{name} row {index}: position_q8 must contain three integers')
        previous = clock
    return rows


def compare(reference, native):
    clocks = [r['elapsed_scale'] for r in reference]
    results = []
    matching = outside = 0
    for row in native:
        clock = row['elapsed_scale']
        result = dict(elapsed_scale=clock, native_position_q8=row['position_q8'])
        if not clocks[0] <= clock <= clocks[-1]:
            result['status'] = 'outside_reference'
            outside += 1
        else:
            upper = bisect_left(clocks, clock)
            lower = upper if clocks[upper] == clock else upper - 1
            a, b = reference[lower], reference[upper]
            weight = Fraction(0) if lower == upper else Fraction(clock - clocks[lower], clocks[upper] - clocks[lower])
            interpolated = [Fraction(x) + weight * (y - x) for x, y in zip(a['position_q8'], b['position_q8'])]
            delta = [Fraction(x) - y for x, y in zip(row['position_q8'], interpolated)]
            equal = all(v == 0 for v in delta)
            matching += equal
            result.update(status='match' if equal else 'different',
                          reference_bracket=[clocks[lower], clocks[upper]],
                          interpolation_weight=[weight.numerator, weight.denominator],
                          reference_position_q8=[float(v) for v in interpolated],
                          delta_q8=[float(v) for v in delta])
        results.append(result)
    return dict(method='piecewise_linear_reference_positions', matching=matching,
                compared=len(native) - outside, outside_reference=outside, results=results)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reference', required=True)
    parser.add_argument('--native', required=True)
    args = parser.parse_args()
    try:
        reference = read_series(args.reference, 'reference')
        native = read_series(args.native, 'native')
    except (ValueError, OSError) as error:
        parser.error(str(error))
    report = compare(reference, native)
    print(json.dumps(report, indent=2))
    return 0 if report['matching'] == len(native) else 1


if __name__ == '__main__':
    raise SystemExit(main())
