#!/usr/bin/env python3
"""Retain a probe's process result separately from its observed emission boundary."""
import json
import math
from pathlib import Path
import subprocess
import time


class CaptureHook:
    def __init__(self, command, output, epoch):
        self.output = Path(output)
        self.output.mkdir(parents=True, exist_ok=True)
        self.epoch = epoch
        self.started = time.monotonic()
        self.stdout = (self.output / 'hook.stdout').open('w')
        self.stderr = (self.output / 'hook.stderr').open('w')
        self.process = subprocess.Popen(command, stdout=self.stdout, stderr=self.stderr)
        self.result = None

    def poll(self):
        code = self.process.poll()
        if code is None:
            return None
        if self.result is None:
            self.result = {'started': self.started, 'completed': time.monotonic(),
                           'returncode': code, 'acknowledged': False}
        return self.result

    def finish(self, required=False, ended=None):
        result = self.poll()
        error = None
        if result is None:
            result = {'started': self.started, 'returncode': None, 'acknowledged': False}
            error = 'capture hook unfinished at recording boundary'
        elif result['returncode'] != 0:
            error = f"capture hook failed with exit {result['returncode']}"
        else:
            self.stdout.flush()
            try:
                event = json.loads((self.output / 'hook.stdout').read_text())
                boundary = event['boundary']
                before, after, game = (boundary[k] for k in ('before', 'after', 'game_seconds'))
                if (event.get('acknowledged') is not True or boundary['event'] != 'sound_emitted'
                        or any(type(v) not in (int, float) or not math.isfinite(v) for v in (before, after, game))
                        or not self.started <= before <= after <= (result['completed'] if ended is None else ended) or game < 0
                        or type(boundary['frame']) is not int or boundary['frame'] < 0
                        or not isinstance(boundary['scene'], str) or not boundary['scene']
                        or not isinstance(boundary['owner'], str) or not boundary['owner']
                        or type(boundary['effect']) is not int):
                    raise ValueError('invalid observed sound-emission boundary')
                result.update(acknowledged=True, boundary=boundary,
                              recording_wall_bounds=[before-self.epoch, after-self.epoch])
            except (ValueError, KeyError, TypeError) as failure:
                error = f'capture hook has no valid observed emission: {failure}'
        if error:
            result['error'] = error
        (self.output / 'hook-result.json').write_text(json.dumps(result, indent=2)+'\n')
        if error and (required or result['returncode'] not in (0, None)):
            raise ValueError(error)
        return result

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=2)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=2)
        self.stdout.close()
        self.stderr.close()
