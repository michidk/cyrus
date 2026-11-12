import importlib.util
from pathlib import Path
import unittest
from PIL import Image
spec = importlib.util.spec_from_file_location('presentation', Path(__file__).with_name('cinematic-presented-frames.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class Presentation(unittest.TestCase):
    def setUp(self):
        self.references = {1: Image.new('RGBA', (640, 480), (255, 0, 0, 255)),
                           2: Image.new('RGBA', (640, 480), (0, 255, 0, 255))}
        self.surfaces = [{'id': i, 'metadata': {'monotonicTimestamp': 100 + i},
                          'published_frame': 99} for i in range(3)]
        self.images = {0: self.references[1], 1: self.references[1], 2: self.references[2]}

    def test_status_progress_cannot_relabel_a_retained_gpu_image(self):
        result = m.identify(self.surfaces, self.images, self.references)
        self.assertEqual([r['frames'] for r in result], [[1], [1], [2]])
        self.assertEqual(result[1]['swap_seconds'], 101)

    def test_identical_images_keep_all_possible_identities(self):
        self.references[3] = self.references[1].copy()
        self.assertEqual(m.identify(self.surfaces, self.images, self.references)[0]['frames'], [1, 3])

    def test_altered_pixel_is_not_admitted_with_a_tolerance(self):
        self.images[1] = self.images[1].copy()
        self.images[1].putpixel((0, 0), (254, 0, 0, 255))
        self.assertEqual(m.identify(self.surfaces, self.images, self.references)[1]['frames'], [])

    def test_loading_surface_and_missing_or_reversed_swap_clocks_fail(self):
        with self.assertRaisesRegex(ValueError, 'progressing'):
            m.identify(self.surfaces, {i: self.references[1] for i in range(3)}, self.references)
        for clock in [None, float('nan'), 99]:
            self.surfaces[1]['metadata']['monotonicTimestamp'] = clock
            with self.assertRaisesRegex(ValueError, 'swap clock'):
                m.identify(self.surfaces, self.images, self.references)


if __name__ == '__main__':
    unittest.main()
