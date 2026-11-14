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
        self.assertEqual(result[1]['expected_display_seconds'], 101)

    def test_identical_images_keep_all_possible_identities(self):
        self.references[3] = self.references[1].copy()
        self.assertEqual(m.identify(self.surfaces, self.images, self.references)[0]['frames'], [1, 3])

    def test_altered_pixel_is_not_admitted_with_a_tolerance(self):
        self.images[1] = self.images[1].copy()
        self.images[1].putpixel((0, 0), (254, 0, 0, 255))
        self.assertEqual(m.identify(self.surfaces, self.images, self.references)[1]['frames'], [])

    def test_published_endpoint_cannot_admit_the_old_retained_image(self):
        with self.assertRaisesRegex(ValueError, 'compositor swap'):
            m.require_endpoint([{'frames': [10], 'published_frame': 1520}], 1520)
        m.require_endpoint([{'frames': [1519]}, {'frames': [1520]}], 1520)

    def test_encoding_arrival_reversal_retains_both_orders_and_raw_clocks(self):
        self.surfaces = [self.surfaces[0], self.surfaces[2], self.surfaces[1]]
        ordered, reversals = m.presentation_order(self.surfaces)
        self.assertEqual([s['id'] for s in ordered], [0, 1, 2])
        self.assertEqual([s['id'] for s in self.surfaces], [0, 2, 1])
        self.assertEqual(reversals, [{'previous_id': 2, 'id': 1,
                                     'previous_seconds': 102, 'seconds': 101}])
        result = m.identify(self.surfaces, self.images, self.references)
        self.assertEqual([s['arrival_sequence'] for s in result], [0, 2, 1])

    def test_duplicate_clock_and_frame_identity_remain_rejected(self):
        self.surfaces[1]['metadata']['monotonicTimestamp'] = 100
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            m.identify(self.surfaces, self.images, self.references)

    def test_installed_subtitle_changes_exact_oracle_only_in_authored_interval(self):
        glyph = {'enabled': True, 'width': 1, 'height': 1, 'offset': [0, 0], 'pixels': [1]}
        metadata = {'font': {'start': 65, 'line_height': 1, 'glyphs': [glyph],
                             'palette': [[0, 0, 0], [63, 0, 0]]},
                    'subtitle_palette': [[0, 0, 0], [253, 0, 0]] + [[0, 0, 0]] * 254,
                    'definition': {'subtitles': [{'start': 2, 'stop': 4, 'always': False, 'text': 'A'}]}}
        raw = Image.new('RGBA', (640, 240), (0, 0, 0, 255))
        blank = Image.new('RGBA', (640, 480), (0, 0, 0, 255))
        image = m.compose(raw, metadata, 2, subtitles=True, scanlines=False, padding_colour=[0, 0, 0])
        self.assertEqual(image.getpixel((319, 469)), (181, 0, 0, 255))
        self.assertEqual(raw.getpixel((319, 239)), (0, 0, 0, 255))
        self.assertEqual(m.compose(raw, metadata, 4, True, False, [0, 0, 0]).tobytes(), blank.tobytes())
        self.assertEqual(m.compose(raw, metadata, 2, False, False, [0, 0, 0]).tobytes(), blank.tobytes())
        metadata['definition']['subtitles'][0]['always'] = True
        self.assertEqual(m.compose(raw, metadata, 2, False, False, [0, 0, 0]).tobytes(), image.tobytes())

    def test_alignment_requires_independently_bounded_movie_clock_returns(self):
        def stamp(message, ts):
            return {'name': 'TimeStamp', 'ts': ts, 'args': {'data': {'message': message}}}
        trace = [stamp('movie-time-begin:0', 990000), stamp('movie-time-end:0', 995000),
                 stamp('movie-time-begin:1', 1010000), stamp('movie-time-end:1', 1015000)]
        pictures = [{'id': 3, 'frames': [15], 'expected_display_seconds': 1}]
        result = m.align_movie_clock(pictures, trace, [{'value': .99}, {'value': 1.01}], 66660)
        self.assertEqual(result[0]['clock_call_before'], (.99, .995, .99))
        with self.assertRaisesRegex(ValueError, 'no bounded'):
            m.align_movie_clock(pictures, [], [{'value': 1}], 66660)
        with self.assertRaisesRegex(ValueError, 'duplicate'):
            m.align_movie_clock(pictures, trace + trace, [{'value': .99}, {'value': 1.01}], 66660)

    def test_loading_surface_and_missing_clocks_fail(self):
        with self.assertRaisesRegex(ValueError, 'progressing'):
            m.identify(self.surfaces, {i: self.references[1] for i in range(3)}, self.references)
        for clock in [None, float('nan')]:
            self.surfaces[1]['metadata']['monotonicTimestamp'] = clock
            with self.assertRaisesRegex(ValueError, 'swap clock'):
                m.identify(self.surfaces, self.images, self.references)


if __name__ == '__main__':
    unittest.main()
