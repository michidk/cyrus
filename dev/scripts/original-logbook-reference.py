#!/usr/bin/env python3
"""Admit a synthetic journal reference through a running original game's log key.

The external, local-only state reader supplies frame, suspended, log_binding,
log_list and keys fields. No executable layout belongs in this capture tool.
"""
import argparse
import json
from pathlib import Path
import subprocess
import time


def capture(container, run, reader, screenshot, *, seconds=20, interval=0.05, key='l'):
    deadline = time.monotonic() + seconds
    events = []

    def execute(*arguments):
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            raise TimeoutError('original logbook reference: admission deadline expired')
        return subprocess.run(['docker', 'exec', '-e', 'DISPLAY=:1', container, *arguments],
                              check=True, capture_output=True, text=True, timeout=remaining)

    def state():
        value = json.loads(execute('python3', reader).stdout)
        events.append(value)
        return value

    before = state()
    while before['frame'] <= 0 or before['suspended']:
        time.sleep(interval)
        before = state()
    while state()['frame'] <= before['frame']:
        time.sleep(interval)
    # New Game has already cleared its journal. Refresh DOSBox's cached directory
    # after creating this fixture through the host mount, before the game opens it.
    path = Path(run) / 'Redguard' / 'LOGBOOK.TXT'
    if path.is_symlink():
        raise ValueError(f'original logbook reference: refuses linked journal: {path}')
    path.write_bytes(b'7,RASTER REFERENCE\r\n')
    execute('xdotool', 'key', 'ctrl+F4')
    opening = state()
    binding = str(opening['log_binding'])
    execute('xdotool', 'keydown', key)
    try:
        while True:
            current = state()
            if current['suspended'] and current['log_list'] and not current['keys'][binding]:
                break
            time.sleep(interval)
    finally:
        # Release even on admission failure; never let an expired polling budget
        # strand a physical key, or allow typematic to close an admitted reader.
        subprocess.run(['docker', 'exec', '-e', 'DISPLAY=:1', container,
                        'xdotool', 'keyup', key], check=True, timeout=2,
                       capture_output=True, text=True)
    released = state()
    if not released['suspended'] or not released['log_list'] or released['keys'][binding]:
        raise ValueError('original logbook reference: reader closed or log key remains held')
    execute(*screenshot)
    after = state()
    if not after['suspended'] or after['log_list'] != released['log_list']:
        raise ValueError('original logbook reference: reader changed during capture')
    return {'success': True, 'fixture': '7,RASTER REFERENCE', 'events': events}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--container', required=True)
    parser.add_argument('--run', type=Path, required=True, help='host-mounted writable run copy')
    parser.add_argument('--reader', required=True, help='local-only state reader inside the container')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--key', default='l', help='installed log key as an X11 key name')
    parser.add_argument('--seconds', type=float, default=20)
    parser.add_argument('screenshot', nargs=argparse.REMAINDER,
                        help='container screenshot command, after --')
    args = parser.parse_args()
    command = args.screenshot
    if command[:1] == ['--']:
        command = command[1:]
    if not command:
        parser.error('a container screenshot command is required')
    result = capture(args.container, args.run, args.reader, command, seconds=args.seconds, key=args.key)
    args.output.write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
