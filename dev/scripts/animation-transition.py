#!/usr/bin/env python3
"""Arm capture before an installed quick-sword transition and admit observed events.

The executable adapter takes one JSON request on stdin and returns one JSON reply.
observe returns frame, cumulative game_seconds, unique world/player owner, group,
and observed events (sequence, owner, kind=play/stop, effect for play, slot).
arm acknowledges armed/event_cursor/pcm_cursor; key receives the installed scan
code and held state; finish finalizes PCM and returns frames and sample_rate.
The adapter owns real event observation and PCM capture. A group crossing alone
never establishes sound emission. Each request must finish within five seconds.
"""
import argparse
import configparser
import fcntl
import hashlib
import json
import math
from pathlib import Path
import subprocess
import time


def capture(observe, arm, key, finish, group, effect, game_limit, wall_limit, persist):
    deadline = time.monotonic() + wall_limit
    receipt = dict(status='waiting', observations=[], events=[])
    def sample():
        if time.monotonic() >= deadline:
            raise ValueError('transition wall deadline expired')
        row = observe()
        if (type(row.get('frame')) is not int or row['frame'] < 0
                or type(row.get('game_seconds')) not in (int, float)
                or not math.isfinite(row['game_seconds']) or row['game_seconds'] < 0
                or not row.get('owner')):
            raise ValueError('transition observer lacks player/game-clock identity')
        if receipt['observations']:
            previous = receipt['observations'][-1]
            if (row['owner'] != previous['owner'] or row['frame'] < previous['frame']
                    or row['game_seconds'] < previous['game_seconds']):
                raise ValueError('transition player/world or clock replaced')
        receipt['observations'].append(row)
        persist(receipt)
        return row
    first = sample()
    while True:
        baseline = sample()
        if baseline['frame'] - first['frame'] >= 2:
            break
        time.sleep(.01)
    if baseline['group'] == group:
        raise ValueError('requested transition already active before capture')
    armed = arm()
    if (armed.get('armed') is not True or type(armed.get('event_cursor')) is not int
            or type(armed.get('pcm_cursor')) is not int):
        raise ValueError('event/PCM capture was not armed')
    receipt.update(status='armed', armed=armed, baseline=baseline)
    persist(receipt)
    entered = None
    held = False
    events = {}
    try:
        held = True
        key(True)
        while True:
            row = sample()
            for event in row.get('events', []):
                sequence = event.get('sequence')
                if type(sequence) is not int:
                    raise ValueError('observed event sequence absent')
                if sequence > armed['event_cursor']:
                    if sequence in events and events[sequence] != event:
                        raise ValueError('observed event identity changed')
                    events[sequence] = event
            if row['group'] == group and entered is None:
                entered = row
                receipt.update(status='entered', entered=row)
                key(False)
                held = False
            if entered is not None and row['group'] != group:
                receipt['completed'] = row
                break
            if row['game_seconds'] - baseline['game_seconds'] >= game_limit:
                raise ValueError('requested transition not observed to completion within game progress')
            time.sleep(.01)
    finally:
        if held:
            key(False)
        receipt['pcm'] = finish()
        receipt['events'] = [events[index] for index in sorted(events)]
        persist(receipt)
    owned = [event for event in receipt['events'] if event.get('owner') == baseline['owner']]
    plays = [event for event in owned if event.get('kind') == 'play' and event.get('effect') == effect]
    stops = [event for event in owned if event.get('kind') == 'stop']
    if (len(plays) != 1 or len(stops) != 1 or plays[0].get('slot') is None
            or plays[0]['slot'] != stops[0].get('slot')
            or plays[0]['sequence'] >= stops[0]['sequence']):
        raise ValueError('selected transition lacks ordered observed emission/stop evidence')
    pcm = receipt['pcm']
    if type(pcm.get('frames')) is not int or pcm['frames'] <= 0 or pcm.get('sample_rate', 0) <= 0:
        raise ValueError('transition PCM capture incomplete')
    receipt['status'] = 'captured'
    persist(receipt)
    return receipt


def binding(path):
    ini = configparser.ConfigParser(interpolation=None, allow_no_value=True)
    with Path(path).open() as source:
        ini.read_file(source)
    return ini.getint('input', 'quick_sword_key')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('adapter', type=Path)
    parser.add_argument('keys_ini', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--group', required=True, type=int, help='installed selected-player transition group')
    parser.add_argument('--effect', required=True, type=int, help='installed group PlaySound operand')
    parser.add_argument('--game-seconds', type=float, default=6)
    parser.add_argument('--deadline', type=float, default=90)
    args = parser.parse_args()
    for value in (args.game_seconds, args.deadline):
        if not math.isfinite(value) or value <= 0:
            parser.error('progress/deadline limits must be positive and finite')
    scan_code = binding(args.keys_ini)
    def request(action, **fields):
        result = subprocess.run([str(args.adapter.resolve())], input=json.dumps(dict(action=action, **fields)),
                                capture_output=True, text=True, check=True, timeout=5)
        return json.loads(result.stdout)
    latest = {}
    def persist(receipt):
        latest.clear()
        latest.update(receipt)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        temporary = args.output.with_suffix('.tmp')
        temporary.write_text(json.dumps(dict(receipt, binding=dict(scan_code=scan_code,
            source=str(args.keys_ini.resolve()), sha256=hashlib.sha256(args.keys_ini.read_bytes()).hexdigest())), indent=2)+'\n')
        temporary.replace(args.output)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.with_suffix('.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:
            capture(lambda: request('observe'), lambda: request('arm'),
                    lambda held: request('key', scan_code=scan_code, held=held), lambda: request('finish'),
                    args.group, args.effect, args.game_seconds, args.deadline, persist)
        except Exception as error:
            persist(dict(latest, status='rejected', error=str(error)))
            raise


if __name__ == '__main__':
    main()
