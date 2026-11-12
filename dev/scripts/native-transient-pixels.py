#!/usr/bin/env python3
"""Assert transient GPU output through bounded public screenshot readback.

Requires Pillow and blake3. Fixtures and readbacks remain local. This tests a
controlled central crop, not original pixels or the exact admission frame.
"""
import argparse
from copy import deepcopy
import fcntl
import importlib.util
import json
from pathlib import Path

spec = importlib.util.spec_from_file_location('transient', Path(__file__).with_name('native-transient-session.py'))
transient = importlib.util.module_from_spec(spec)
spec.loader.exec_module(transient)
capture = transient.capture


def pixels(path):
    from PIL import Image
    with Image.open(path) as image:
        image.load()
        w, h = image.size
        # The fixture is centered ahead of the captured camera. Exclude HUD,
        # weapon and the horizon; do not learn a mask from unrelated scenery.
        crop = image.convert('RGB').crop((w * 3 // 8, h // 3, w * 5 // 8, h * 2 // 3))
        return crop.size, list(crop.getdata())


def signal(empty, visible):
    if empty[0] != visible[0]:
        raise ValueError('GPU readback dimensions changed')
    mask = [i for i, (a, b) in enumerate(zip(empty[1], visible[1]))
            if max(abs(x - y) for x, y in zip(a, b)) >= 24]
    if len(mask) < 64:
        raise ValueError('controlled transient has no sufficient visible GPU signal')
    return mask


def assert_output(empty, positive, observed, mask, present):
    if empty[0] != observed[0]:
        raise ValueError('GPU readback dimensions changed')
    target = positive if present else empty
    matched = sum(max(abs(x - y) for x, y in zip(target[1][i], observed[1][i])) <= 8 for i in mask)
    fraction = matched / len(mask)
    if fraction < .98:
        raise ValueError(f'transient GPU output mismatch: present={present}, matched={fraction}')
    return {'signal_pixels': len(mask), 'matched_fraction': fraction, 'present': present}


class PixelDriver(transient.TransientDriver):
    def shot(self, name):
        # Screenshot's public completion is asynchronous GPU copy/map plus PNG
        # write. The inherited bounded wait rejects stale/truncated output.
        self.screenshot(name + '.png')
        return pixels(self.output / (name + '.png'))

    def publish(self, name, data):
        capture.atomic(self.output / (name + '.session'), data)
        self.load(name, str(Path(self.args.runtime_output) / (name + '.session')))
        self.progress(self.ready())

    def identity(self):
        return {'binary': capture.file_digest(self.args.binary),
                'seed': capture.file_digest(self.args.seed_session),
                'driver': capture.file_digest(Path(__file__)),
                'capture': capture.file_digest(Path(capture.__file__)),
                'transient': capture.file_digest(Path(transient.__file__)),
                'scene': self.args.scene, 'game_seconds': self.args.game_seconds}

    def checkpoint(self, results, image):
        files = {p.name: capture.file_digest(p) for p in self.output.iterdir()
                 if p.suffix in ('.session', '.png')}
        self.save('gpu-checkpoint', {'identity': self.identity(), 'files': files,
                                    'results': results, 'image': image})
        self.save('gpu-progress', results)

    def resume(self):
        path = self.output / 'gpu-checkpoint.json'
        if not path.exists():
            return {}, None
        saved = json.loads(path.read_text())
        if saved['identity'] != self.identity():
            raise ValueError('GPU checkpoint inputs changed; use a fresh output directory')
        for name, checksum in saved['files'].items():
            if capture.file_digest(self.output / name) != checksum:
                raise ValueError(f'GPU checkpoint evidence changed: {name}')
        return saved['results'], saved['image']

    def run(self):
        self.ready()
        results, image = self.resume()
        fixture = capture.document(self.args.seed_session.read_text())
        pool = transient.current(fixture)['effects']
        if not pool['slots'][0] or pool['slots'][0]['speed'] != 0:
            raise ValueError('GPU fixture requires a retained stationary transient')
        empty_document = deepcopy(fixture)
        transient.current(empty_document)['effects']['slots'] = [None] * len(pool['slots'])
        empty_document['render_playback']['textures']['effect_draws'] = []
        if 'visible' not in results:
            self.publish('empty', transient.encode(empty_document))
            self.shot('empty')
            self.publish('visible', transient.encode(fixture))
            geometry = self.observe()['geometry']
            descriptor = pool['slots'][0]['descriptor'] & 0xffff
            image = next(row for row in geometry['images'] if row['descriptor'] == descriptor)
            self.request('hold_texture_frame', world=self.args.scene, image=image['name'], frame=0)
            self.progress(self.ready())
            self.shot('visible')
        empty, positive = pixels(self.output / 'empty.png'), pixels(self.output / 'visible.png')
        mask = signal(empty, positive)
        if 'visible' not in results:
            results['visible'] = assert_output(empty, positive, positive, mask, True)
            self.checkpoint(results, image)
        # Each incomplete stage establishes its own public live-state precondition.
        # A restarted process must not depend on the retired process's world.
        if 'restored' not in results or 'rejected' not in results:
            self.publish('restored', transient.encode(fixture))
            self.request('hold_texture_frame', world=self.args.scene, image=image['name'], frame=0)
            self.progress(self.ready())
            if 'restored' not in results:
                results['restored'] = assert_output(empty, positive, self.shot('restored'), mask, True)
                self.checkpoint(results, image)
        if 'rejected' not in results:
            invalid = deepcopy(fixture)
            transient.current(invalid)['effects']['source'] = '0' * 64
            capture.atomic(self.output / 'invalid.session', transient.encode(invalid))
            before = self.observe()
            self.reject(str(Path(self.args.runtime_output) / 'invalid.session'), before)
            try:
                self.request('session_load_result', acknowledge=True)
            except RuntimeError as error:
                if 'saved environmental effect source changed' not in str(error):
                    raise
            results['rejected'] = assert_output(empty, positive, self.shot('rejected'), mask, True)
            self.checkpoint(results, image)
        if 'retired' not in results:
            self.publish('retired', transient.encode(empty_document))
            results['retired'] = assert_output(empty, positive, self.shot('retired'), mask, False)
            self.checkpoint(results, image)
        if 'expired' not in results:
            ending = deepcopy(fixture)
            transient.current(ending)['effects']['slots'][0].update(frame=image['frames'] - 1, speed=1)
            self.publish('expired', transient.encode(ending))
            results['expired'] = assert_output(empty, positive, self.shot('expired'), mask, False)
            self.checkpoint(results, image)
        self.save('result', {'gpu_readback': results, 'binary_sha256': capture.file_digest(self.args.binary),
                             'driver_sha256': capture.file_digest(Path(__file__))})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('port', type=int)
    parser.add_argument('output', type=Path)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--runtime-output', required=True)
    parser.add_argument('--seed-session', required=True, type=Path)
    parser.add_argument('--scene', default='ISLAND')
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
            PixelDriver(args).run()
        except Exception as error:
            capture.atomic(args.output / 'failure.json', json.dumps({'error': str(error)}).encode())
            raise


if __name__ == '__main__':
    main()
