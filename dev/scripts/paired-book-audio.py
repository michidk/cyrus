#!/usr/bin/env python3
"""Pair live isolated unity, public book input and an independent clock observer.

Run inside a private paced runtime. The clock command emits JSON lines with
monotonic before/after bounds and game_seconds. The transition command emits
ordinary paced-mixer-capture acknowledgements and owns its bounded audio tail.
"""
import argparse
import fcntl
import importlib.util
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import time


def module(name, filename):
    spec = importlib.util.spec_from_file_location(name, Path(__file__).with_name(filename))
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def streams():
    return json.loads(subprocess.run(['pactl', '--format=json', 'list', 'sink-inputs'],
                                    check=True, capture_output=True, text=True, timeout=5).stdout)


def producer(values, binary):
    found = [value for value in values
             if value.get('properties', {}).get('application.process.binary') == binary]
    if len(found) != 1 or found[0].get('mute') is not False:
        raise ValueError('require exactly one unmuted owned producer: '+binary)
    return found[0]


def settings(value):
    return {key: value[key] for key in ('index', 'sink', 'mute', 'volume', 'sample_specification', 'channel_map')}


def calibrate(output, binary):
    """Mute only the named runtime producer while measuring the known signal."""
    before = producer(streams(), binary)
    record = {'before': settings(before), 'started': time.monotonic()}
    try:
        subprocess.run(['pactl', 'set-sink-input-mute', str(before['index']), '1'], check=True, timeout=5)
        module('monitor', 'paced-monitor-check.py').run(output / 'calibration', None, 20)
    finally:
        subprocess.run(['pactl', 'set-sink-input-mute', str(before['index']), '0'], check=True, timeout=5)
        record['restored'] = time.monotonic()
        after = producer(streams(), binary)
        record['after'] = settings(after)
        (output / 'calibration-settings.json').write_text(json.dumps(record, indent=2)+'\n')
    if record['before'] != record['after']:
        raise ValueError('producer settings changed during calibration')
    return record


def drive(output, binary, clock, command):
    output.mkdir(parents=True, exist_ok=True)
    with (output / 'clocks.jsonl').open('x') as clocks, (output / 'clock.log').open('w') as errors:
        observer = subprocess.Popen(shlex.split(clock), stdout=clocks, stderr=errors)
        try:
            calibrate(output, binary)
            if observer.poll() is not None:
                raise RuntimeError('clock observer exited before transition')
            subprocess.run(command, check=True, timeout=45)
            if observer.poll() is not None:
                raise RuntimeError('clock observer exited during transition')
        finally:
            observer.terminate()
            try:
                observer.wait(timeout=5)
            except subprocess.TimeoutExpired:
                observer.kill(); observer.wait()
                raise RuntimeError('clock observer did not stop with transition')
    # Do not wait for a fixed-duration observer before stopping PCM. That adds
    # unrelated repeated cues to the acknowledged transition's recording.


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('mode', choices=['capture', 'drive'])
    parser.add_argument('output', type=Path)
    parser.add_argument('--producer', required=True, help='exact owned application.process.binary')
    parser.add_argument('--clock-command', required=True)
    parser.add_argument('--ready-command')
    parser.add_argument('command', nargs=argparse.REMAINDER)
    # Options precede positionals because the transition is an opaque command.
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if not command:
        parser.error('transition command required')
    if args.mode == 'drive':
        drive(args.output, args.producer, args.clock_command, command)
    else:
        if not args.ready_command:
            parser.error('capture requires --ready-command')
        driver = [sys.executable, str(Path(__file__).resolve()), '--producer', args.producer,
                  '--clock-command', args.clock_command, 'drive', str(args.output), '--', *command]
        args.output.mkdir(parents=True, exist_ok=True)
        with (args.output / 'pair.lock').open('a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            module('capture', 'paced-mixer-capture.py').capture(
                args.output / 'capture', driver, 90, .25, shlex.split(args.ready_command))


if __name__ == '__main__':
    main()
