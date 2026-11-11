#!/usr/bin/env python3
"""Drive a bounded original rope contact run inside the retro container.

Start DOSBox-X with -set "dosbox hostkey=ctrlalt" (Ctrl+Alt host modifier).
Requires the ignored original-state.py reader and a local placement command. The
placement command receives no arguments and must place the player near the chosen
installed rope; executable-specific diagnostics never belong in this script.
Run under the host live-run lock. stdout is JSONL, including rejected baselines.
Console text holds each key and its release for at least two measured frame
intervals because the original polls key state. Gameplay holds
end after observed world updates, with a wall-clock cutoff if the game stalls.
"""
import argparse
import json
import subprocess
import sys
import time


class Driver:
    def __init__(self, reader, timeout, pulse):
        self.reader = reader
        self.timeout = timeout
        self.pulse = pulse
        self.held = set()

    def input(self, action, key):
        subprocess.run(['xdotool', action, key], check=True, timeout=5)
        if action == 'keydown':
            self.held.add(key)
        else:
            self.held.discard(key)

    def release(self):
        for key in tuple(self.held):
            self.input('keyup', key)

    def key(self, key):
        self.input('keydown', key)
        try:
            time.sleep(self.pulse)
        finally:
            self.input('keyup', key)
        time.sleep(self.pulse)

    def command(self, text):
        self.key('F12')
        try:
            opened = self.sample('console-open')
            require_interactive_console(opened)
            before = opened['frame']
            time.sleep(2 * self.pulse)
            settled = self.sample('console-open')
            require_interactive_console(settled)
            after = settled['frame']
            if before != after:
                raise RuntimeError('console did not pause world updates; refusing gameplay text')
            for char in text:
                self.key({' ': 'space', '=': 'equal', ',': 'comma'}.get(char, char))
            self.key('Return')
        finally:
            # Escape leaves the console without reopening it or requesting a load.
            self.key('Escape')

    def sample(self, phase):
        result = subprocess.run([sys.executable, self.reader], check=True,
                                capture_output=True, text=True, timeout=self.timeout)
        state = json.loads(result.stdout)
        player, = [a for a in state['actors'] if a['selected']]
        row = dict(phase=phase, frame=state['counters']['mainloop_frame'],
                   counters=state['counters'], player=player,
                   console_mode=state.get('console_mode'))
        print(json.dumps(row), flush=True)
        return row

    def ready(self, timeout):
        deadline = time.monotonic() + timeout
        while True:
            try:
                row = self.sample('ready')
                if row['frame'] > 2:
                    return row
            except (subprocess.CalledProcessError, ValueError):
                pass
            if time.monotonic() >= deadline:
                raise RuntimeError('original selected player did not become ready')
            time.sleep(.1)

    def progress(self, frames, phase, validate=False):
        start = self.sample(phase)['frame']
        deadline = time.monotonic() + self.timeout
        previous = start
        while True:
            row = self.sample(phase)
            if validate:
                baseline(row)
            if row['frame'] < previous:
                raise RuntimeError('world clock restarted')
            previous = row['frame']
            if row['frame'] - start >= frames:
                return row
            if time.monotonic() >= deadline:
                raise RuntimeError('original world clock stalled')
            time.sleep(.05)


def require_interactive_console(row):
    if row['console_mode'] != 'interactive':
        raise RuntimeError('console is not interactive; refusing gameplay text')


def baseline(row):
    player = row['player']
    if not player['drawn'] or player['falling'] or player['living'] != 0:
        raise RuntimeError(f"invalid contact baseline: {row}")
    if player['health'] <= 0:
        raise RuntimeError('selected player is dead')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--reader', required=True)
    parser.add_argument('--place', nargs='+', required=True,
                        help='local placement command, run after sword preparation')
    parser.add_argument('--frames', type=int, default=16)
    parser.add_argument('--timeout', type=float, default=35)
    parser.add_argument('--ready-timeout', type=float, default=100)
    parser.add_argument('--pulse', type=float, default=.2)
    parser.add_argument('--attack-key', default='Control_L')
    args = parser.parse_args()
    if args.frames <= 0 or args.timeout <= 0 or args.ready_timeout <= 0 or args.pulse <= 0:
        parser.error('frames, timeouts and pulse must be positive')
    driver = Driver(args.reader, args.timeout, args.pulse)
    try:
        ready = driver.ready(args.ready_timeout)
        window = subprocess.check_output(['xdotool', 'search', '--name', 'DOSBox'], text=True).splitlines()[-1]
        subprocess.run(['xdotool', 'windowfocus', '--sync', window], check=True, timeout=5)
        fps = ready['counters'].get('fps', 0)
        if fps > 0:
            driver.pulse = max(driver.pulse, 2 / fps)
        driver.progress(2, 'ready')
        driver.command('magiccarpet = 1')
        driver.command('item add,1')
        if not driver.sample('inventory')['player']['drawn']:
            driver.input('keydown', 's')
            try:
                driver.progress(2, 'draw')
            finally:
                driver.input('keyup', 's')
        driver.progress(12, 'settle')
        subprocess.run(args.place, check=True, timeout=args.timeout)
        row = driver.progress(2, 'placed')
        baseline(row)
        driver.input('keydown', 'Up')
        try:
            driver.progress(args.frames, 'walk-in', validate=True)
        finally:
            driver.input('keyup', 'Up')
        driver.input('keydown', args.attack_key)
        try:
            driver.progress(2, 'attack', validate=True)
        finally:
            driver.input('keyup', args.attack_key)
        driver.progress(12, 'continuation', validate=True)
    finally:
        driver.release()


if __name__ == '__main__':
    main()
