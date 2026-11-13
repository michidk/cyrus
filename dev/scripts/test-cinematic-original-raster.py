import importlib.util
from pathlib import Path
import unittest

import numpy as np
from PIL import Image

spec = importlib.util.spec_from_file_location('raster', Path(__file__).with_name('cinematic-original-raster.py'))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class OriginalRaster(unittest.TestCase):
    def test_fixed_setup_anchor_cannot_be_replaced_by_left_edge_ratio(self):
        texture = np.zeros((2, 256, 3), dtype=np.int64)
        texture[:, 127] = 255
        # The setup vertex anchors at the right edge. Rounding a gradient does
        # not permit moving that anchor to the left edge of the rectangle.
        draw = {'destination': [0, 0, 257, 1], 'anchor': [257, 0],
                'start': [256 << 32, 0], 'dx': [int(np.float32(256 / 257) * 2**32), 0],
                'dy': [0, 1 << 32]}
        anchored = m.sample(texture, draw)
        moved = dict(draw, anchor=[0, 0], start=[0, 0])
        self.assertFalse(np.array_equal(anchored, m.sample(texture, moved)))

    def test_sampling_wraps_through_padding_and_truncates_fraction(self):
        texture = np.zeros((2, 2, 3), dtype=np.int64)
        texture[0, 0] = 240
        draw = {'destination': [0, 0, 1, 1], 'anchor': [0, 0],
                'start': [0, 1 << 31], 'dx': [0, 0], 'dy': [0, 0]}
        self.assertEqual(m.sample(texture, draw).tolist(), [[[120, 120, 120]]])
        draw['start'][0] = (1 << 31) + (15 << 24)
        self.assertEqual(m.sample(texture, draw).tolist(), [[[240, 240, 240]]])
        draw['start'][0] += 1 << 24
        self.assertEqual(m.sample(texture, draw).tolist(), [[[225, 225, 225]]])

    def test_dither_uses_absolute_pixel_position_before_565_storage(self):
        colour = np.full((1, 1, 3), 100)
        self.assertEqual(m.store565(colour, 0, 0).tolist(), [[[99, 97, 99]]])
        self.assertEqual(m.store565(colour, 1, 0).tolist(), [[[99, 101, 99]]])
        self.assertFalse(np.array_equal(m.store565(colour), colour))

    def test_full_frame_rejection_includes_last_pixel(self):
        expected = Image.new('RGBA', (640, 480), (0, 0, 0, 255))
        self.assertEqual(m.require_equal(expected, expected)['pixels'], 307200)
        changed = expected.copy()
        changed.putpixel((639, 479), (1, 0, 0, 255))
        with self.assertRaisesRegex(ValueError, '1 pixels'):
            m.require_equal(changed, expected)

    def test_subtitle_palette_and_stored_background_are_both_authoritative(self):
        glyph = {'enabled': True, 'width': 1, 'height': 1, 'offset': [0, 0], 'pixels': [1]}
        metadata = {'font': {'start': 65, 'line_height': 1, 'glyphs': [glyph],
                             'palette': [[0, 0, 0], [39, 3, 14]]},
                    'definition': {'subtitles': [{'start': 2, 'stop': 4, 'always': False, 'text': 'A'}]}}
        background = Image.new('RGBA', (640, 480), (99, 101, 99, 255))
        palette = [[0, 0, 0], [159, 14, 56]] + [[0, 0, 0]] * 254
        composed = m.compose_subtitles(background, metadata, 2, palette)
        self.assertEqual(composed.getpixel((319, 469)), (140, 40, 66, 255))
        self.assertEqual(m.compose_subtitles(background, metadata, 4, palette).tobytes(), background.tobytes())
        wrong = [c[:] for c in palette]
        wrong[1] = [156, 12, 56]
        with self.assertRaisesRegex(ValueError, 'full raster'):
            m.require_equal(m.compose_subtitles(background, metadata, 2, wrong), composed)
        self.assertEqual(m.compose_subtitles(background, metadata, 2, palette, False).tobytes(), background.tobytes())

    def test_invalid_backing_and_unclipped_destination_are_rejected(self):
        draw = {'source': [0, 0, 1, 1], 'backing': [3, 2],
                'destination': [0, 0, 1, 1], 'anchor': [0, 0],
                'start': [1 << 31, 1 << 31], 'dx': [0, 0], 'dy': [0, 0]}
        with self.assertRaisesRegex(ValueError, 'power-of-two'):
            m.compose(Image.new('RGB', (1, 1)), [0, 0, 0], [draw])
        draw['destination'][2] = 641
        with self.assertRaisesRegex(ValueError, 'clipping'):
            m.compose(Image.new('RGB', (1, 1)), [0, 0, 0], [draw])


if __name__ == '__main__':
    unittest.main()
