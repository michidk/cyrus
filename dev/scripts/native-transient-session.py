#!/usr/bin/env python3
"""Exercise installed transient geometry through public diagnostic session replacement.

The fixture selects an installed descriptor at speed zero to isolate
publication from lifetime. This is a diagnostic continuation fixture, not a claim
about an installed producer's timing or original pixel parity.
"""
import argparse
from copy import deepcopy
import fcntl
import importlib.util
import json
from pathlib import Path
import socket

spec = importlib.util.spec_from_file_location('session_capture', Path(__file__).with_name('native-session-capture.py'))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


def encode(value):
    from blake3 import blake3
    payload = json.dumps(value, separators=(',', ':')).encode()
    return b'CYRUS-SESSION 1\n' + blake3(payload).hexdigest().encode() + b'\n' + payload


def current(snapshot):
    registry = snapshot['logical']['registry']
    return next(world for world in registry['worlds'] if world['world'] == registry['current'])


def fixture(data, geometry, position=None):
    snapshot = capture.document(data.decode())
    pool = current(snapshot)['effects']
    retained = next((effect for effect in pool['slots'] if effect), None)
    if retained is None:
        if position is None:
            raise ValueError('fixture requires an installed producer or observed actor position')
        image = next((image for image in sorted(geometry['images'], key=lambda row: row['descriptor'])
                      if image['frames'] > 1), None)
        if image is None:
            raise ValueError('fixture requires an installed expiring effect image')
        retained = {'descriptor': image['descriptor'], 'position': position}
    image = next(image for image in geometry['images'] if image['descriptor'] == (retained['descriptor'] & 0xffff))
    if image['frames'] < 2:
        raise ValueError('fixture requires an installed effect with an expiring lifetime')
    # Ordinary drawing isolates geometry from alternate-path per-draw movement.
    descriptor = image['descriptor']
    effect = {'descriptor': descriptor, 'position': position if position is not None else retained['position'], 'frame': 0, 'speed': 0}
    pool['slots'] = [effect] + [None] * (len(pool['slots']) - 1)
    textures = snapshot['render_playback']['textures']
    textures['effect_draws'] = [{'slot': 0, 'descriptor': descriptor, 'position': effect['position'], 'frame': 0}]
    playback = next(row for row in textures['images'] if row['image'].lower() == image['name'].lower())
    playback.update(frame=0, held=True)
    return snapshot, image


def view_position(snapshot):
    """Place the diagnostic quad ten scene units ahead of the captured public view."""
    pose = snapshot['camera']['pose']
    x, y, z, w = pose['rotation']
    # Rotate local -Z by the captured quaternion, then invert exported scene units.
    direction = [-2 * (x * z + w * y), -2 * (y * z - w * x), -(1 - 2 * (x * x + y * y))]
    point = [value + 10 * forward for value, forward in zip(pose['translation'], direction)]
    return [round(-point[0] * 20), round(-point[1] * 20), round(point[2] * 20 + 65535.99609375)]


def assert_geometry(before, after, replaced):
    old, new = before['geometry'], after['geometry']
    for record in (old, new):
        if len(record['retained']) != 1 or len(record['logical']) != 1:
            raise ValueError('fixture must retain exactly one logical and submitted effect')
        slot = record['slots'][0]
        if (not slot['admitted'] or len(slot['vertices']) != 4 or not slot['texture']
                or not slot['texture_hash'] or slot['alpha'] <= 0 or slot['visibility'] != 'Some(Visible)'):
            raise ValueError('transient geometry was not published')
        camera = record['camera_rotation']
        if camera is None or any(abs(a - b) > 1e-5 for a, b in zip(slot['rotation'], camera)):
            raise ValueError('transient billboard does not face the current camera')
        if not any(any(component != 0 for component in vertex) for vertex in slot['vertices']):
            raise ValueError('transient geometry remains a placeholder')
    for field in ('world', 'retained', 'logical'):
        if old[field] != new[field]:
            raise ValueError(f'transient {field} continuation changed')
    for field in ('vertices', 'translation', 'texture_hash', 'alpha', 'visibility'):
        if old['slots'][0][field] != new['slots'][0][field]:
            raise ValueError(f'transient rendered {field} changed')
    if (old['generation'] != new['generation']) != replaced:
        raise ValueError('transient generation does not match publication outcome')
    if replaced:
        if set(old['entities']) & set(new['entities']):
            raise ValueError('old transient entities survived replacement')
        for field in ('mesh', 'material'):
            if old['slots'][0][field] == new['slots'][0][field]:
                raise ValueError(f'old transient {field} survived replacement')
    else:
        if any(abs(a - b) > 1e-5 for a, b in zip(old['camera_rotation'], new['camera_rotation'])):
            raise ValueError('transient inspection view changed')
        left, right = deepcopy(old), deepcopy(new)
        for record in (left, right):
            record.pop('camera_rotation')
            for slot in record['slots']:
                slot.pop('rotation')
        # Unsubmitted pool placeholders acquire camera rotation on idle repaint.
        # Their rotation is not output; every binding and submitted pose still matters.
        if left != right or any(
                (a['admitted'] or b['admitted']) and any(abs(x - y) > 1e-5
                    for x, y in zip(a['rotation'], b['rotation']))
                for a, b in zip(old['slots'], new['slots'])):
            raise ValueError('rejected load changed transient bindings')


class TransientDriver(capture.ContinuationDriver):
    def observe(self):
        return self.request('effect_geometry')

    def arm(self, generation):
        connection = socket.create_connection(('127.0.0.1', self.args.port), timeout=2)
        connection.settimeout(self.args.prepare_seconds)
        connection.sendall((json.dumps({'id': 7, 'cmd': {
            'type': 'effect_geometry', 'after_generation': generation}}) + '\n').encode())
        try:
            self.wait(lambda: self.request('state').get('effect_geometry_pending'), self.args.seconds)
            return connection
        except Exception:
            connection.close()
            raise

    def first(self, connection):
        try:
            reply = json.loads(connection.makefile('rb').readline())
            if not reply.get('ok'):
                raise RuntimeError(reply.get('error', reply))
            return reply['result']
        finally:
            connection.close()

    def load(self, name, runtime_path):
        self.request('console', open=True)
        try:
            super().load(name, runtime_path, prepare_seconds=self.args.prepare_seconds)
        finally:
            self.request('console', open=False)

    def reject(self, runtime_path, before):
        """Sample the live binding during preparation as well as after rejection."""
        self.phase('reject-transient-source')
        samples = []
        self.request('console', open=True)
        try:
            self.request('session_load', path=runtime_path)
            def poll():
                observed = self.observe()
                assert_geometry(before, observed, False)
                samples.append(observed)
                try:
                    terminal = self.request('session_load_result')
                except RuntimeError as error:
                    if 'saved environmental effect source changed' not in str(error):
                        raise
                    return str(error)
                if not terminal.get('pending', True):
                    raise ValueError('invalid effect source was published')
                return None
            return self.wait(poll, self.args.prepare_seconds), samples
        finally:
            self.request('console', open=False)

    def save(self, name, value):
        capture.atomic(self.output / (name + '.json'), json.dumps(value, indent=2).encode())

    def identity(self):
        return {'binary': capture.file_digest(self.args.binary),
                'driver': capture.file_digest(Path(__file__)),
                'capture_driver': capture.file_digest(Path(capture.__file__)),
                'scene': self.args.scene, 'dormant': self.args.dormant,
                'game_seconds': self.args.game_seconds}

    def checkpoint(self, stage):
        state_path = self.output / ('stage-' + stage + '.session')
        capture.atomic(state_path, self.capture())
        files = {path.name: capture.file_digest(path) for path in self.output.iterdir()
                 if path.suffix in ('.session', '.json', '.png')
                 and path.name not in ('checkpoint.json', 'phase.json', 'failure.json', 'result.json')}
        capture.atomic(self.output / 'checkpoint.json', json.dumps({
            'identity': self.identity(), 'stage': stage, 'files': files}, indent=2).encode())

    def resume(self):
        path = self.output / 'checkpoint.json'
        if not path.exists():
            return None
        checkpoint = json.loads(path.read_text())
        if checkpoint['identity'] != self.identity():
            raise ValueError('checkpoint executable/driver/options changed; use a fresh output directory')
        for name, checksum in checkpoint['files'].items():
            if capture.file_digest(self.output / name) != checksum:
                raise ValueError(f'checkpoint evidence changed: {name}')
        self.load('resume-' + checkpoint['stage'], str(Path(self.args.runtime_output) /
                                                     ('stage-' + checkpoint['stage'] + '.session')))
        self.progress(self.ready())
        return checkpoint['stage']

    def run(self):
        self.ready()
        stages = ['prepared', 'published', 'rejected', 'revisited', 'expired']
        resumed = self.resume()
        done = stages.index(resumed) if resumed else -1
        if done < 0:
            if self.args.seed_session:
                data = self.args.seed_session.read_bytes()
                snapshot = capture.document(data.decode())
                capture.atomic(self.output / 'fixture.session', data)
            else:
                state = self.ready()
                for scene in (self.args.dormant, self.args.scene):
                    if state.get('scene', '').upper() != scene.upper():
                        self.request('load_scene', name=scene)
                    state = self.progress(self.ready(scene.upper()))
                self.phase('prepare-installed-transient')
                data = self.capture()
                geometry = self.observe()['geometry']
                capture.atomic(self.output / 'installed.session', data)
                self.request('flycam', enabled=True)
                self.progress(self.ready())
                data = self.capture()
                position = view_position(capture.document(data.decode()))
                snapshot, image = fixture(data, self.observe()['geometry'], position)
                capture.atomic(self.output / 'fixture.session', encode(snapshot))
            self.load('bootstrap-transient', str(Path(self.args.runtime_output) / 'fixture.session'))
            self.progress(self.ready())
            before = self.observe()
            self.save('before', before)
            assert_geometry(before, before, False)
            self.screenshot('before.png')
            capture.atomic(self.output / 'before.session', self.capture())
            descriptor = current(snapshot)['effects']['slots'][0]['descriptor'] & 0xffff
            image = next(row for row in before['geometry']['images'] if row['descriptor'] == descriptor)
            self.save('fixture-image', image)
            self.checkpoint('prepared')
        image = json.loads((self.output / 'fixture-image.json').read_text())
        snapshot = capture.document((self.output / 'fixture.session').read_text())
        if done < 1:
            before = self.observe()
            self.phase('publish-transient')
            armed = self.arm(before['geometry']['generation'])
            try:
                self.load('publish-transient', str(Path(self.args.runtime_output) / 'before.session'))
                published = self.first(armed)
            finally:
                armed.close()
            self.save('published-first-update', published)
            assert_geometry(before, published, True)
            self.screenshot('published.png')
            self.progress(self.ready())
            stable = self.observe()
            self.save('published-settled', stable)
            assert_geometry(published, stable, False)
            self.checkpoint('published')
        if done < 2:
            stable = self.observe()
            invalid = capture.document((self.output / 'before.session').read_text())
            current(invalid)['effects']['source'] = '0' * 64
            capture.atomic(self.output / 'rejected.session', encode(invalid))
            rejection, rejection_samples = self.reject(str(Path(self.args.runtime_output) / 'rejected.session'), stable)
            rejected = self.observe()
            assert_geometry(stable, rejected, False)
            try:
                self.request('session_load_result', acknowledge=True)
            except RuntimeError as error:
                if 'saved environmental effect source changed' not in str(error):
                    raise
            self.save('rejected', {'error': rejection, 'samples': rejection_samples, 'before': stable, 'after': rejected})
            self.checkpoint('rejected')
        if done < 3:
            stable = self.observe()
            before = stable
            self.phase('dormant-transient')
            self.request('load_scene', name=self.args.dormant)
            self.progress(self.ready(self.args.dormant.upper()))
            dormant = self.observe()
            if set(stable['geometry']['entities']) & set(dormant['geometry']['entities']):
                raise ValueError('old transient geometry survived world retirement')
            dormant_capture = capture.document(self.capture().decode())
            saved = next(world for world in dormant_capture['logical']['registry']['worlds']
                         if world['world'] == before['geometry']['world'])
            if saved['effects']['slots'][0] != current(snapshot)['effects']['slots'][0]:
                raise ValueError('dormant logical transient changed')
            self.save('dormant', dormant)
            armed = self.arm(dormant['geometry']['generation'])
            try:
                self.request('load_scene', name=self.args.scene)
                revisited = self.first(armed)
            finally:
                armed.close()
            self.save('revisited-first-update', revisited)
            assert_geometry(before, revisited, True)
            self.progress(self.ready(self.args.scene.upper()))
            self.screenshot('revisited.png')
            self.checkpoint('revisited')
        if done < 4:
            # Resume the installed lifetime boundary instead of manufacturing a new policy.
            ending = capture.document(self.capture().decode())
            effect = current(ending)['effects']['slots'][0]
            effect.update(frame=image['frames'] - 1, speed=1)
            capture.atomic(self.output / 'ending.session', encode(ending))
            self.load('expire-transient', str(Path(self.args.runtime_output) / 'ending.session'))
            self.progress(self.ready())
            expired = self.observe()
            self.save('expired', expired)
            if expired['geometry']['logical'] or expired['geometry']['retained'] or any(
                    slot['admitted'] for slot in expired['geometry']['slots']):
                raise ValueError('expired transient remains submitted')
            self.screenshot('transient-final.png')
            self.checkpoint('expired')
        self.save('result', {'public_round_trip': True, 'first_update_geometry': True,
                  'old_geometry_removed': True, 'rejection_preserved': True,
                  'dormant_revisit': True, 'lifetime_removal': True,
                  'binary_sha256': capture.file_digest(self.args.binary),
                  'driver_sha256': capture.file_digest(Path(__file__)),
                  'resumed_after': resumed})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('port', type=int)
    parser.add_argument('output', type=Path)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--runtime-output', required=True)
    parser.add_argument('--seed-session', type=Path, help='reuse an explicit retained installed diagnostic fixture')
    parser.add_argument('--scene', default='ISLAND')
    parser.add_argument('--dormant', default='START')
    parser.add_argument('--seconds', type=float, default=180)
    parser.add_argument('--prepare-seconds', type=float, default=600)
    parser.add_argument('--game-seconds', type=float, default=.4)
    parser.add_argument('--interval', type=float, default=.1)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with (args.output / 'capture.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        for name in ('result.json', 'failure.json'):
            (args.output / name).unlink(missing_ok=True)
        try:
            TransientDriver(args).run()
        except Exception as error:
            capture.atomic(args.output / 'failure.json', json.dumps({
                'error': str(error), 'type': type(error).__name__}).encode())
            raise


if __name__ == '__main__':
    main()
