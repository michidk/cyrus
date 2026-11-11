#!/usr/bin/env python3
"""Capture and resume a rendered public session round trip without game-data fixtures."""
import argparse
import fcntl
import hashlib
import json
import os
from pathlib import Path
import socket
import time


def digest(data):
    return hashlib.sha256(data).hexdigest()


def file_digest(path):
    checksum = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            checksum.update(block)
    return checksum.hexdigest()


def atomic(path, data):
    temporary = path.with_suffix(path.suffix + '.tmp')
    with temporary.open('wb') as stream:
        stream.write(data)
        stream.flush()
        os.fsync(stream.fileno())
    temporary.replace(path)
    descriptor = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def document(text):
    header, checksum, payload = text.split('\n', 2)
    if header != 'CYRUS-SESSION 1' or len(checksum) != 64:
        raise ValueError('unsupported session framing')
    int(checksum, 16)
    value = json.loads(payload)
    if value['version'] != 1:
        raise ValueError('unsupported current session schema')
    return value


def compare_sources(before, after):
    """Check immutable world identities, leaving animation and pixel parity explicit."""
    def inventory(snapshot):
        registry = snapshot['logical']['registry']
        if registry['version'] != 1 or len(registry['worlds']) < 2:
            raise ValueError('current-schema capture must retain both visited worlds')
        worlds = {world['world']: (world['content_id'], world['source_identity'])
                  for world in registry['worlds']}
        generations = {world['generation']: len(world['actors']) for world in registry['worlds']}
        for binding in snapshot['entity_bindings']:
            actor = binding.get('actor')
            if actor is not None and (actor[0] not in generations
                                      or not 0 <= actor[1] < generations[actor[0]]):
                raise ValueError('rendered actor binding references an absent world/actor')
        for field in ('player_model', 'actor_models', 'render_playback'):
            if snapshot.get(field) is None:
                raise ValueError(f'rendered capture lacks {field}')
        return worlds
    original, restored = inventory(before), inventory(after)
    if original != restored:
        raise ValueError('restoration changed retained world content/source identity')
    return {'retained_worlds': len(original), 'world_sources_preserved': True,
            'rendered_actor_bindings_valid': True}


class Driver:
    def __init__(self, args):
        self.args = args
        self.output = args.output

    def phase(self, name):
        atomic(self.output / 'phase.json', json.dumps({'phase': name, 'wall_time': time.time()}).encode())

    def request(self, kind, **fields):
        with socket.create_connection(('127.0.0.1', self.args.port), timeout=2) as connection:
            connection.settimeout(self.args.seconds)
            connection.sendall((json.dumps({'id': 1, 'cmd': {'type': kind, **fields}}) + '\n').encode())
            reply = json.loads(connection.makefile('rb').readline())
        if not reply.get('ok'):
            raise RuntimeError(f'{kind}: {reply.get("error", reply)}')
        return reply['result']

    def wait(self, operation, seconds):
        deadline = time.monotonic() + seconds
        last = None
        while time.monotonic() < deadline:
            try:
                result = operation()
                if result:
                    return result
            except (OSError, json.JSONDecodeError) as error:
                last = str(error)
            time.sleep(self.args.interval)
        raise TimeoutError(f'bounded operation expired; last transport error: {last}')

    def ready(self, scene=None):
        def poll():
            state = self.request('state')
            with (self.output / 'timeline.jsonl').open('a') as stream:
                stream.write(json.dumps(state) + '\n')
            return state if (state.get('state') == 'InGame' and state.get('scene_ready') and state.get('clock')
                             and (scene is None or state.get('scene', '').upper() == scene)) else None
        # Preparation, including movie export, has its own deadline. It is not a game-clock stall.
        return self.wait(poll, self.args.prepare_seconds)

    def progress(self, state):
        initial = state['clock']['game_seconds']
        def poll():
            current = self.request('state')
            clock = current.get('clock')
            return current if (current.get('scene_ready') and clock
                               and clock['game_seconds'] >= initial + self.args.game_seconds) else None
        return self.wait(poll, self.args.seconds)

    def capture(self):
        self.request('session_capture')
        result = self.wait(lambda: self.terminal('session_capture_result'), self.args.seconds)
        document(result['document'])
        return result['document'].encode()

    def terminal(self, kind):
        result = self.request(kind)
        return result if not result.get('pending', True) else None

    def screenshot(self, name):
        path = self.output / name
        path.unlink(missing_ok=True)
        self.request('screenshot', path=str(Path(self.args.runtime_output) / name))
        def complete():
            if not path.exists() or path.stat().st_size < 20:
                return False
            with path.open('rb') as stream:
                signature = stream.read(8)
                stream.seek(-12, os.SEEK_END)
                return (signature == b'\x89PNG\r\n\x1a\n'
                        and stream.read() == b'\x00\x00\x00\x00IEND\xaeB`\x82')
        self.wait(complete, self.args.seconds)

    def run(self):
        checkpoint = self.output / 'checkpoint.json'
        binary = file_digest(self.args.binary)
        resumed = checkpoint.exists()
        if resumed:
            saved = json.loads(checkpoint.read_text())
            data = (self.output / 'before.session').read_bytes()
            if saved != {'version': 1, 'binary_sha256': binary, 'session_sha256': digest(data)}:
                raise ValueError('checkpoint binary or session identity mismatch')
            document(data.decode())
        else:
            for scene in ('START', 'ISLAND'):
                self.phase('visit-' + scene)
                self.ready()
                self.request('load_scene', name=scene)
                self.progress(self.ready(scene))
            self.phase('capture-before')
            data = self.capture()
            atomic(self.output / 'before.session', data)
            atomic(checkpoint, json.dumps({'version': 1, 'binary_sha256': binary,
                                          'session_sha256': digest(data)}).encode())
            self.phase('capture-before-image')
            self.screenshot('before.png')
        # Always retry publication from the durable source after interruption, even if
        # the previous process died after commit but before recording its result.
        self.phase('publish-saved-session')
        self.ready()
        if resumed:
            # A timed-out driver may have left a completed result in the same
            # live runtime. A fresh process reports no terminal result instead.
            try:
                self.request('session_load_result', acknowledge=True)
            except RuntimeError as error:
                if 'has no terminal result' not in str(error):
                    raise
        self.request('session_load', path=str(Path(self.args.runtime_output) / 'before.session'))
        result = self.wait(lambda: self.terminal('session_load_result'), self.args.seconds)
        if result.get('committed') is not True:
            raise RuntimeError(f'publication did not commit: {result}')
        self.request('session_load_result', acknowledge=True)
        self.phase('first-update-continuation')
        state = self.progress(self.ready('ISLAND'))
        self.phase('capture-after')
        restored = self.capture()
        atomic(self.output / 'after.session', restored)
        comparison = compare_sources(document(data.decode()), document(restored.decode()))
        self.phase('rendered-capture')
        self.screenshot('after.png')
        atomic(self.output / 'result.json', json.dumps({
            'source_comparison': comparison,
            'publication': 'committed', 'gameplay_state': state,
            'before_sha256': digest(data), 'after_sha256': digest(restored),
            'presentation_comparison': 'unverified; inspect saved documents and rendered capture',
        }, indent=2).encode())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('port', type=int)
    parser.add_argument('output', type=Path)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--runtime-output', required=True, help='same output directory as seen by the game')
    parser.add_argument('--prepare-seconds', type=float, default=600)
    parser.add_argument('--seconds', type=float, default=180)
    parser.add_argument('--game-seconds', type=float, default=0.3)
    parser.add_argument('--interval', type=float, default=0.2)
    args = parser.parse_args()
    if min(args.prepare_seconds, args.seconds, args.game_seconds, args.interval) <= 0:
        parser.error('time bounds and progress must be positive')
    args.output.mkdir(parents=True, exist_ok=True)
    with (args.output / 'capture.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        # A prior success must never survive a failed retry as a current result.
        (args.output / 'result.json').unlink(missing_ok=True)
        (args.output / 'failure.json').unlink(missing_ok=True)
        try:
            Driver(args).run()
        except Exception as error:
            atomic(args.output / 'failure.json', json.dumps({'error': str(error), 'type': type(error).__name__}).encode())
            raise


if __name__ == '__main__':
    main()
