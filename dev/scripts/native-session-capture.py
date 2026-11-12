#!/usr/bin/env python3
"""Capture and resume a rendered public session round trip without game-data fixtures."""
import argparse
import fcntl
import hashlib
import json
import os
import re
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
        deadline = time.monotonic() + self.args.seconds
        while True:
            self.request('session_capture')
            try:
                result = self.wait(lambda: self.terminal('session_capture_result'),
                                   max(self.args.interval, deadline - time.monotonic()))
                document(result['document'])
                return result['document'].encode()
            except RuntimeError as error:
                # The public boundary rejects a frame whose asynchronous mesh
                # replacement has not settled. Its result was consumed; retry
                # capture without changing animation or accepting stale geometry.
                not_ready = (
                    'displayed model mesh has not settled to the selected frame' in str(error)
                    or re.fullmatch(r'session_capture_result: player model [^:]+: instance is not ready', str(error))
                    or re.fullmatch(r'session_capture_result: dialogue \[\d+(?:, \d+)*\]: export still in flight at save boundary', str(error))
                )
                if not not_ready or time.monotonic() >= deadline:
                    raise
                time.sleep(self.args.interval)

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


class ContinuationDriver(Driver):
    """Restore an existing two-world seed, then observe real attachment continuation."""
    def load(self, name, runtime_path, *, prepare_seconds=None):
        self.phase(name)
        try:
            self.request('session_load_result', acknowledge=True)
        except RuntimeError as error:
            if 'has no terminal result' not in str(error):
                raise
        self.request('session_load', path=runtime_path)
        result = self.wait(lambda: self.terminal('session_load_result'),
                           self.args.seconds if prepare_seconds is None else prepare_seconds)
        if result.get('committed') is not True:
            raise ValueError('continuation publication did not commit')
        self.request('session_load_result', acknowledge=True)

    def observe_change(self, operation, index):
        from concurrent.futures import ThreadPoolExecutor
        generation = self.request('scripts', offset=0, limit=1)['generation']
        with ThreadPoolExecutor(max_workers=1) as executor:
            observed = executor.submit(self.request, 'rendered_actor', index=index,
                                       after_generation=generation)
            self.wait(lambda: self.request('state').get('rendered_actor_pending'), self.args.seconds)
            operation()
            return observed.result(timeout=self.args.seconds)

    def run(self):
        from native_session_assertions import assert_observation, assert_saved_pose, assert_dormant_preserved
        metadata = self.output / 'continuation-checkpoint.json'
        identity = {'version': 1, 'binary_sha256': file_digest(self.args.binary),
                    'seed_sha256': file_digest(self.args.continuation_seed),
                    'actor': self.args.attachment_actor, 'revisit': self.args.revisit,
                    'revisit_actor': self.args.revisit_actor}
        if metadata.exists():
            saved = json.loads(metadata.read_text())
            data = (self.output / 'continuation.session').read_bytes()
            if saved['identity'] != identity or saved['session_sha256'] != digest(data):
                raise ValueError('continuation checkpoint identity mismatch')
            before = saved['before']
        else:
            self.ready()
            self.load('bootstrap-retained-fixture', self.args.runtime_seed)
            self.progress(self.ready())
            before = self.request('rendered_actor', index=self.args.attachment_actor)
            assert_observation(before)
            self.phase('capture-active-attachment')
            data = self.capture()
            assert_saved_pose(document(data.decode()), before)
            atomic(self.output / 'continuation.session', data)
            atomic(metadata, json.dumps({'identity': identity, 'session_sha256': digest(data),
                                        'before': before}).encode())
        snapshot = document(data.decode())
        self.ready()
        first = self.observe_change(
            lambda: self.load('publish-attachment-fixture', str(Path(self.args.runtime_output) / 'continuation.session')),
            self.args.attachment_actor)
        atomic(self.output / 'first-update.json', json.dumps(first, indent=2).encode())
        assert_saved_pose(snapshot, before)
        assert_saved_pose(snapshot, first)
        if before['rendered_actor']['model']['source'] != first['rendered_actor']['model']['source']:
            raise ValueError('restored active actor model source changed')
        if before['rendered_actor']['owner'] == first['rendered_actor']['owner']:
            raise ValueError('restored active actor reused its retired entity owner')
        self.phase('capture-restored-attachment')
        after = self.capture()
        atomic(self.output / 'continuation-after.session', after)
        compare_sources(snapshot, document(after.decode()))
        dormant = next(world for world in snapshot['logical']['registry']['worlds']
                       if world['world'] != snapshot['logical']['registry']['current'])
        assert_dormant_preserved(snapshot, document(after.decode()), dormant['world'])
        revisit = self.observe_change(lambda: self.request('load_scene', name=self.args.revisit),
                                     self.args.revisit_actor)
        atomic(self.output / 'revisit-first-update.json', json.dumps(revisit, indent=2).encode())
        if revisit['rendered_actor']['world'] != dormant['world']:
            raise ValueError('revisit selected a different dormant world')
        assert_saved_pose(snapshot, revisit)
        self.progress(self.ready(self.args.revisit.upper()))
        self.screenshot('continuation-revisit.png')
        atomic(self.output / 'result.json', json.dumps({
            'publication': 'committed', 'active_attachment_continuation': True,
            'dormant_world_preserved': True, 'dormant_revisit_attachment': True,
            'first_update': first, 'revisit_first_update': revisit,
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
    parser.add_argument('--continuation-seed', type=Path)
    parser.add_argument('--runtime-seed')
    parser.add_argument('--attachment-actor', type=int)
    parser.add_argument('--revisit')
    parser.add_argument('--revisit-actor', type=int)
    args = parser.parse_args()
    if min(args.prepare_seconds, args.seconds, args.game_seconds, args.interval) <= 0:
        parser.error('time bounds and progress must be positive')
    continuation = (args.continuation_seed, args.runtime_seed, args.attachment_actor,
                    args.revisit, args.revisit_actor)
    if any(value is not None for value in continuation) and any(value is None for value in continuation):
        parser.error('continuation requires seed/runtime-seed, attachment-actor, revisit and revisit-actor')
    args.output.mkdir(parents=True, exist_ok=True)
    with (args.output / 'capture.lock').open('a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        # A prior success must never survive a failed retry as a current result.
        (args.output / 'result.json').unlink(missing_ok=True)
        (args.output / 'failure.json').unlink(missing_ok=True)
        try:
            (ContinuationDriver(args) if args.continuation_seed else Driver(args)).run()
        except Exception as error:
            atomic(args.output / 'failure.json', json.dumps({'error': str(error), 'type': type(error).__name__}).encode())
            raise


if __name__ == '__main__':
    main()
