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
import hashlib
import fcntl
from contextlib import ExitStack
import os
from pathlib import Path


class Driver:
    def __init__(self, reader, timeout, pulse):
        self.reader = reader
        self.timeout = timeout
        self.pulse = pulse
        self.held = set()
        self.progress_path = None
        self.progress_record = {}

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

    def recorder(self):
        try:
            self.input('keydown', 'ctrl')
            self.input('keydown', 'alt')
            subprocess.run(['xdotool', 'key', 'w'], check=True, timeout=5)
        finally:
            self.input('keyup', 'alt')
            self.input('keyup', 'ctrl')

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

    def journal(self, path, resume, placement):
        identity = dict(reader_sha256=hashlib.sha256(Path(self.reader).read_bytes()).hexdigest(),
                        placement=placement, placement_files={item: hashlib.sha256(Path(item).read_bytes()).hexdigest()
                            for item in placement if Path(item).is_file()})
        self.progress_path = path
        if path.exists():
            if not resume:
                raise ValueError('readiness progress exists; use --resume or a new output')
            record = json.loads(path.read_text())
            if record.get('version') != 1 or record.get('identity') != identity:
                raise ValueError('readiness source changed')
            if record['status'] != 'waiting':
                raise ValueError('input attempt completed or was interrupted; never replay it automatically')
            self.progress_record = record
        else:
            if resume:
                raise ValueError('no readiness progress to resume')
            self.progress_record = dict(version=1, identity=identity, status='waiting', polls=0)
            self.save()

    def save(self):
        if self.progress_path is None:
            return
        self.progress_path.parent.mkdir(parents=True, exist_ok=True)
        temporary = self.progress_path.with_suffix('.tmp')
        with temporary.open('w') as stream:
            json.dump(self.progress_record, stream, indent=2)
            stream.flush()
            os.fsync(stream.fileno())
        temporary.replace(self.progress_path)
        descriptor = os.open(self.progress_path.parent, os.O_RDONLY)
        try:
            os.fsync(descriptor)
        finally:
            os.close(descriptor)

    def mark_running(self):
        self.progress_record['status'] = 'running'
        self.save()

    def observe(self):
        try:
            result = subprocess.run([sys.executable, self.reader], check=True,
                                    capture_output=True, text=True, timeout=self.timeout)
            state = json.loads(result.stdout)
            actors = state['actors']
            selected = [a for a in actors if a['selected']]
            readiness = ('actor_absent' if not actors else 'selection_absent' if not selected
                         else 'selection_ambiguous' if len(selected) != 1 else 'selected')
            return dict(readiness=readiness, actor_count=len(actors), selected_count=len(selected),
                        reader=state.get('readiness'), frame=state['counters']['mainloop_frame'],
                        counters=state['counters'], player=selected[0] if len(selected) == 1 else None,
                        console_mode=state.get('console_mode'),
                        game_seconds=state.get('game_seconds'), clock_end=state.get('clock_end'),
                        input_state=state.get('input_state', {}))
        except subprocess.CalledProcessError as error:
            return dict(readiness='reader_failed', returncode=error.returncode,
                        stderr=error.stderr, stdout=error.stdout)
        except subprocess.TimeoutExpired as error:
            return dict(readiness='reader_timeout', error=str(error))
        except (OSError, ValueError, KeyError, TypeError) as error:
            return dict(readiness='reader_invalid', error=str(error))

    def sample(self, phase):
        row = dict(self.observe(), phase=phase)
        print(json.dumps(row), flush=True)
        if row['readiness'] != 'selected':
            raise RuntimeError(f'original player unavailable: {row}')
        return row

    def boundary(self, phase, edge, key=None):
        before = self.sample(phase)
        if before['game_seconds'] is None or before['clock_end'] is None:
            raise RuntimeError('reader lacks cumulative game clock; refusing timed gameplay')
        if key:
            pressed = edge == 'start'
            self.input('keydown' if pressed else 'keyup', key)
            deadline = time.monotonic() + self.timeout
            while True:
                after = self.sample(phase)
                if after['input_state'].get(key) is pressed:
                    break
                if time.monotonic() >= deadline:
                    raise RuntimeError('original input was not acknowledged')
        else:
            after = before
        row = dict(phase=phase, boundary=edge, input_ack=True,
                   clock=[before['game_seconds'], after['clock_end']], player=after['player'])
        print(json.dumps(row), flush=True)
        return row

    def segment(self, frames, phase, key=None):
        self.boundary(phase, 'start', key)
        try:
            self.progress(frames, phase, validate=True)
        finally:
            self.boundary(phase, 'end', key)

    def ready(self, timeout):
        started = time.monotonic()
        deadline = started + timeout
        while True:
            row = dict(self.observe(), phase='ready', observed_unix_ns=time.time_ns(),
                       readiness_elapsed_seconds=time.monotonic() - started)
            print(json.dumps(row), flush=True)
            self.progress_record['polls'] = self.progress_record.get('polls', 0) + 1
            previous = self.progress_record.get('last_observation', {})
            if any(row.get(key) != previous.get(key) for key in
                   ('readiness', 'frame', 'actor_count', 'selected_count', 'game_seconds')):
                self.progress_record['last_progress_poll'] = self.progress_record['polls']
            self.progress_record['last_observation'] = row
            self.save()
            if row['readiness'] == 'selected' and row['frame'] > 2:
                return row
            if time.monotonic() >= deadline:
                raise RuntimeError(f'original readiness timed out: {row}')
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
    parser.add_argument('--progress', type=Path, help='durable readiness journal')
    parser.add_argument('--resume', action='store_true', help='resume waiting only; never replay input')
    parser.add_argument('--frames', type=int, default=16)
    parser.add_argument('--timeout', type=float, default=35)
    parser.add_argument('--ready-timeout', type=float, default=100)
    parser.add_argument('--pulse', type=float, default=.2)
    parser.add_argument('--record', action='store_true',
                        help='record the DOSBox-X mixer with the Ctrl+Alt host modifier')
    parser.add_argument('--attack-key', default='Control_L')
    args = parser.parse_args()
    if args.frames <= 0 or args.timeout <= 0 or args.ready_timeout <= 0 or args.pulse <= 0:
        parser.error('frames, timeouts and pulse must be positive')
    with ExitStack() as stack:
        if args.progress:
            args.progress.parent.mkdir(parents=True, exist_ok=True)
            lock = stack.enter_context(args.progress.with_suffix('.lock').open('a'))
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        driver = Driver(args.reader, args.timeout, args.pulse)
        if args.resume and args.progress is None:
            parser.error('--resume requires --progress')
        if args.progress:
            driver.journal(args.progress, args.resume, args.place)
        recording = False
        try:
            ready = driver.ready(args.ready_timeout)
            driver.mark_running()
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
            if args.record:
                driver.recorder()
                recording = True
            driver.segment(args.frames, 'walk-in', 'Up')
            driver.segment(2, 'attack', args.attack_key)
            driver.segment(12, 'continuation')
            driver.progress_record['status'] = 'completed'
            driver.save()
        finally:
            driver.release()
            if recording:
                driver.recorder()


if __name__ == '__main__':
    main()
