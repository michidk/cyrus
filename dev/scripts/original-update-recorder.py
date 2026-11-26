#!/usr/bin/env python3
"""Record stopped original ordinary-update boundaries through a local backend.

The backend is an ignored Python module, supplied with --backend. It provides
open_session(), a context manager yielding next_boundary(timeout) and snapshot().
next_boundary resumes to a verified ordinary-update boundary and leaves execution
stopped. snapshot reads without resuming. Executable bindings stay in that module.
A boundary precedes the next ordinary update: its state is the completed previous
update and next_step is the clock scale that the next update will consume.
"""
import argparse
import importlib.util
import json
import sys


FIELDS = ('position_q8', 'animation', 'fall', 'support_owner', 'link', 'previous_local_q8')


def validate_snapshot(row):
    """Reject running, partial or untyped snapshots; null support/link is explicit."""
    if row.get('execution') != 'stopped' or row.get('boundary') != 'ordinary_update_start':
        raise ValueError('snapshot is not a stopped ordinary boundary')
    for name in ('update', 'clock_scale', 'next_step'):
        if type(row.get(name)) is not int or row[name] < 0:
            raise ValueError(f'invalid {name}')
    if row['next_step'] == 0:
        raise ValueError('nonpositive consumed clock step')
    player = row.get('player', {})
    if any(name not in player for name in FIELDS):
        raise ValueError('incomplete player snapshot')
    for name in ('position_q8', 'previous_local_q8'):
        if (not isinstance(player[name], list) or len(player[name]) != 3
                or any(type(x) is not int for x in player[name])):
            raise ValueError(f'invalid {name}')
    for name in ('animation', 'fall'):
        if type(player[name]) is not int:
            raise ValueError(f'invalid {name}')
    for name in ('support_owner', 'link'):
        if player[name] is not None and type(player[name]) is not int:
            raise ValueError(f'invalid {name}')


def record(session, elapsed, emit, timeout=10, *, require_bios=False):
    """Emit baseline then exactly one row per completed update, until clock progress.

    Duplicate counters, gaps, resets, torn reads and clock/consumed-step mismatches
    fail the recording. Partial output must not be used after a nonzero exit.
    timeout bounds a stalled boundary wait, never the requested gameplay span.
    Optional unsigned BIOS samples produce observed intervals, with null at the
    baseline. require_bios rejects missing samples. Resets/wrap are unsupported;
    no interval is inferred from game-clock scales or host time.
    """
    if type(elapsed) is not int or elapsed <= 0:
        raise ValueError('elapsed clock span must be positive')
    previous = None
    origin = None
    sequence = 0
    while True:
        session.next_boundary(timeout)
        row = session.snapshot()
        validate_snapshot(row)
        bios = row.get('bios_ticks')
        if require_bios or 'bios_ticks' in row or (previous is not None and 'bios_ticks' in previous):
            if type(bios) is not int or not 0 <= bios <= 2**32 - 1:
                raise ValueError('observed unsigned BIOS sample unavailable')
            if previous is not None and ('bios_ticks' not in previous or bios < previous['bios_ticks']):
                raise ValueError('BIOS sample reset or baseline unavailable')
        if session.snapshot() != row:
            raise ValueError('snapshot changed while execution was stopped')
        if previous is None:
            origin = row['clock_scale']
            consumed = 0
        else:
            if row['update'] != previous['update'] + 1:
                raise ValueError('update counter did not advance exactly once')
            consumed = row['clock_scale'] - previous['clock_scale']
            if consumed != previous['next_step']:
                raise ValueError('game clock differs from consumed update step')
        timing = {} if bios is None else dict(
            elapsed_bios_ticks=None if previous is None else bios - previous['bios_ticks'])
        emit(dict(sequence=sequence, elapsed_scale=row['clock_scale'] - origin,
                  consumed_clock_step=consumed, **timing, **row))
        if row['clock_scale'] - origin >= elapsed:
            return sequence
        previous = row
        sequence += 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--backend', required=True, help='ignored executable-specific backend module')
    parser.add_argument('--elapsed', required=True, type=int, help='consumed game-clock units')
    parser.add_argument('--timeout', type=float, default=10, help='boundary stall deadline in seconds')
    parser.add_argument('--require-bios', action='store_true',
                        help='require observed unsigned BIOS samples at every stopped boundary')
    args = parser.parse_args()
    spec = importlib.util.spec_from_file_location('original_update_backend', args.backend)
    backend = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(backend)
    with backend.open_session() as session:
        record(session, args.elapsed, lambda row: print(json.dumps(row), flush=True), args.timeout, require_bios=args.require_bios)


if __name__ == '__main__':
    try:
        main()
    except (ValueError, RuntimeError, TimeoutError) as error:
        print(f'original update recording rejected: {error}', file=sys.stderr)
        sys.exit(1)
