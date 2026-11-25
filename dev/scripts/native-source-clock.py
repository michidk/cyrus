#!/usr/bin/env python3
"""Check a live mono source's fractional host clock through public session captures.

Start with one active finite mono voice, then supply explicit host intervals.
This checks decoder progress, not original mixer-buffer or gameplay parity.
"""
import argparse
import importlib.util
import json
from pathlib import Path
import socket


def helper(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


replay = helper('source_clock_replay', 'replay-chain.py')
capture = helper('source_clock_capture', 'native-session-capture.py')


def source(snapshot, slot):
    """Resolve a public channel slot to exactly one live source owner."""
    samples = snapshot['samples']
    slots = samples['ownership']['samples']['slots']
    if not 0 <= slot < len(slots) or slots[slot] is None:
        raise ValueError(f'sample slot {slot}: expected an active source')
    binding = slots[slot]
    players = [player for player in samples['players'] if player['token'] == binding['token']]
    if len(players) != 1:
        raise ValueError(f'sample slot {slot}: expected exactly one bound player')
    player = players[0]
    transport = player['transport']
    if player['paused'] or transport['speed'] != 1.0 or transport['repetitions'] != 1:
        raise ValueError(f'sample slot {slot}: requires an unpaused finite source at unit speed')
    # Mono Panned emits left then right. Reading left advances the input cursor;
    # a pending right sample has not yet been consumed by the host.
    output_samples = 2 * transport['input_samples'] - int(transport['right_bits'] is not None)
    return {'token': binding['token'], 'entity': player['entity'],
            'source_identity': transport['source_identity'], 'owner': binding.get('owner'),
            'output_samples': output_samples}


def assert_progress(initial, current, elapsed_nanos, sample_rate):
    """Require exact stereo-output progress; carry fractional samples across intervals."""
    for field in ('token', 'entity', 'source_identity', 'owner'):
        if initial[field] != current[field]:
            raise ValueError(f'source clock: {field} changed during the measured interval')
    expected = elapsed_nanos * sample_rate * 2 // 1_000_000_000
    actual = current['output_samples'] - initial['output_samples']
    if expected != actual:
        raise ValueError(f'source clock: {elapsed_nanos} ns at {sample_rate} Hz: '
                         f'expected {expected} output samples, observed {actual}')
    return {'elapsed_nanos': elapsed_nanos, 'expected_output_samples': expected,
            'observed_output_samples': actual}


def run(control, output, slot, sample_rate, intervals):
    output.mkdir(parents=True, exist_ok=False)

    def observe(index):
        path = output / f'source-{index}.session'
        replay.capture_session(control, path)
        return source(capture.document(path.read_text()), slot)

    initial = observe(0)
    if initial['output_samples'] != 0:
        raise ValueError('source clock: restore a zero-cursor mono source before measuring')
    elapsed = 0
    observations = []
    for index, interval in enumerate(intervals, 1):
        control.request({'type': 'replay_frame', 'frame_scale': 0,
                         'elapsed_bios_ticks': 0, 'elapsed_host_nanos': interval})
        elapsed += interval
        observations.append(assert_progress(initial, observe(index), elapsed, sample_rate))
    capture.atomic(output / 'source-clock.json', json.dumps(observations, indent=2).encode())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('port', type=int)
    parser.add_argument('output', type=Path)
    parser.add_argument('--slot', type=int, required=True)
    parser.add_argument('--sample-rate', type=int, required=True)
    parser.add_argument('--interval-nanos', type=int, nargs='+', required=True)
    args = parser.parse_args()
    if args.sample_rate <= 0 or any(interval < 0 for interval in args.interval_nanos):
        parser.error('sample rate must be positive and intervals must be nonnegative')
    with socket.create_connection(('127.0.0.1', args.port), timeout=120) as connection:
        run(replay.Control(connection), args.output, args.slot, args.sample_rate, args.interval_nanos)


if __name__ == '__main__':
    main()
