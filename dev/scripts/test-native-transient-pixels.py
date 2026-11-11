#!/usr/bin/env python3
"""Mutation controls for paired GPU output, independent of installed art."""
import importlib.util
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

spec = importlib.util.spec_from_file_location('pixels', Path(__file__).with_name('native-transient-pixels.py'))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class PixelControls(unittest.TestCase):
    def test_missing_stale_and_retained_output_fail(self):
        empty = ((10, 10), [(10, 20, 30)] * 100)
        visible = ((10, 10), [(100, 150, 200)] * 100)
        mask = module.signal(empty, visible)
        for observed, present in ((visible, True), (empty, False)):
            self.assertEqual(module.assert_output(empty, visible, observed, mask, present)['matched_fraction'], 1)
        for observed, present in ((empty, True), (visible, False)):
            with self.subTest(present=present), self.assertRaisesRegex(ValueError, 'output mismatch'):
                module.assert_output(empty, visible, observed, mask, present)
        with self.assertRaisesRegex(ValueError, 'no sufficient visible'):
            module.signal(empty, empty)
        with self.assertRaisesRegex(ValueError, 'dimensions'):
            module.assert_output(empty, visible, ((1, 100), empty[1]), mask, True)

    def test_retired_checkpoint_resumes_only_expiration_and_guards_evidence(self):
        from PIL import Image
        from types import SimpleNamespace
        import json
        with TemporaryDirectory() as temporary:
            output = Path(temporary)
            binary = output / 'binary'
            binary.write_bytes(b'unchanged executable')
            fixture = {'version': 1, 'logical': {'registry': {'current': 1, 'worlds': [
                {'world': 1, 'effects': {'slots': [{'speed': 0, 'descriptor': 3, 'frame': 0}]}}]}},
                'render_playback': {'textures': {'effect_draws': []}}}
            seed = output / 'seed.session'
            seed.write_bytes(module.transient.encode(fixture))
            for name, color in [('empty', (10, 20, 30)), ('visible', (100, 150, 200))]:
                Image.new('RGB', (320, 240), color).save(output / (name + '.png'))
            args = SimpleNamespace(output=output, binary=binary, seed_session=seed,
                                   scene='ISLAND', game_seconds=.4)
            driver = module.PixelDriver(args)
            results = {key: {'complete': True} for key in ('visible', 'restored', 'rejected', 'retired')}
            driver.checkpoint(results, {'frames': 4, 'name': 'installed'})
            driver.ready = lambda: {}
            publishes = []
            driver.publish = lambda name, data: publishes.append(name)
            driver.shot = lambda name: module.pixels(output / 'empty.png')
            driver.run()
            self.assertEqual(publishes, ['expired'])
            self.assertEqual(len(json.loads((output / 'result.json').read_text())['gpu_readback']), 5)
            (output / 'visible.png').write_bytes(b'changed evidence')
            with self.assertRaisesRegex(ValueError, 'evidence changed'):
                driver.resume()
            binary.write_bytes(b'different executable')
            with self.assertRaisesRegex(ValueError, 'inputs changed'):
                driver.resume()

    def test_real_png_decode_crop_and_truncation(self):
        from PIL import Image
        with TemporaryDirectory() as temporary:
            path = Path(temporary) / 'readback.png'
            image = Image.new('RGB', (320, 240), (10, 20, 30))
            image.save(path)
            self.assertEqual(module.pixels(path), ((80, 80), [(10, 20, 30)] * 6400))
            path.write_bytes(path.read_bytes()[:40])
            with self.assertRaises(OSError):
                module.pixels(path)


if __name__ == '__main__':
    unittest.main()
