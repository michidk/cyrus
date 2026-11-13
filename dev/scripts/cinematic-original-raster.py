#!/usr/bin/env python3
"""Reconstruct a captured indexed cinematic draw from explicit raster state.

The state is supplied by the original-runtime observer, not inferred from the
captured picture. Coordinates and gradients are signed 32.32 texel values.
The DOSBox software Voodoo profile uses four fractional filter bits, wrapping
texture backing, integer bilinear filtering and dithered RGB565 storage.
"""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path

import numpy as np
from PIL import Image

DITHER = np.array([[0, 8, 2, 10], [12, 4, 14, 6],
                   [3, 11, 1, 9], [15, 7, 13, 5]])


def store565(colours, left=0, top=0):
    """Quantize at absolute destination coordinates, then expand stored bits."""
    colours = np.asarray(colours, dtype=np.int64)
    yy, xx = np.indices(colours.shape[:2])
    dither = DITHER[(yy + top) % 4, (xx + left) % 4, None]
    adjusted = np.empty_like(colours)
    rb = colours[..., [0, 2]]
    adjusted[..., [0, 2]] = (2 * rb - (rb >> 4) + (rb >> 7) + dither) >> 1
    green = colours[..., 1:2]
    adjusted[..., 1:2] = (4 * green - (green >> 4) + (green >> 6) + dither) >> 2
    stored = adjusted >> np.array([3, 2, 3])
    return (stored << np.array([3, 2, 3])) | (stored >> np.array([2, 4, 2]))


def sample(texture, rectangle):
    """Sample one axis-aligned draw using its observed setup anchor/gradients.

    `destination` is an exclusive rectangle. `anchor` is the setup vertex in
    integer destination pixels; `start` and `dx`/`dy` contain S,T in 32.32.
    Texture dimensions must be powers of two; wrapping includes padded texels.
    """
    texture = np.asarray(texture, dtype=np.int64)
    height, width, channels = texture.shape
    if channels != 3 or min(width, height) < 1 or width & (width - 1) or height & (height - 1):
        raise ValueError('texture backing must have power-of-two RGB dimensions')
    left, top, right, bottom = rectangle['destination']
    if right <= left or bottom <= top:
        raise ValueError('empty destination rectangle')
    yy, xx = np.indices((bottom - top, right - left))
    xx += left - rectangle['anchor'][0]
    yy += top - rectangle['anchor'][1]
    coordinates = []
    for axis in range(2):
        dx, dy = rectangle['dx'][axis], rectangle['dy'][axis]
        value = rectangle['start'][axis] + xx * dx + yy * dy + ((dx + dy) >> 1)
        coordinates.append((value >> 24) - 128)
    sx, sy = coordinates
    x, y = sx >> 8, sy >> 8
    fx, fy = (sx & 240)[..., None], (sy & 240)[..., None]
    a = texture[y & (height - 1), x & (width - 1)]
    b = texture[y & (height - 1), (x + 1) & (width - 1)]
    c = texture[(y + 1) & (height - 1), x & (width - 1)]
    d = texture[(y + 1) & (height - 1), (x + 1) & (width - 1)]
    upper = a + ((b - a) * fx >> 8)
    lower = c + ((d - c) * fx >> 8)
    return upper + ((lower - upper) * fy >> 8)


def compose(decoded, padding_colour, draws, size=(640, 480)):
    """Build the full raster, including explicit source padding and draw overlap.

    `draws` are in submission order and include the observer's clipping bounds.
    No edge pixels are masked after comparison and no fitting is performed.
    """
    decoded = np.asarray(decoded.convert('RGB'), dtype=np.int64)
    output = np.zeros((size[1], size[0], 3), dtype=np.int64)
    for draw in draws:
        x, y, width, height = draw['source']
        tw, th = draw['backing']
        if min(x, y) < 0 or width <= 0 or height <= 0 or x + width > decoded.shape[1] or y + height > decoded.shape[0] or width > tw or height > th:
            raise ValueError('source rectangle exceeds decoded image or texture backing')
        texture = np.empty((th, tw, 3), dtype=np.int64)
        texture[:] = padding_colour
        texture[:height, :width] = decoded[y:y + height, x:x + width]
        left, top, right, bottom = draw['destination']
        if not (0 <= left < right <= size[0] and 0 <= top < bottom <= size[1]):
            raise ValueError('destination must include observed clipping')
        output[top:bottom, left:right] = store565(sample(texture, draw), left, top)
    return Image.fromarray(output.astype(np.uint8), 'RGB').convert('RGBA')


def compose_subtitles(image, metadata, frame, palette, enabled=True, right=640):
    """Composite glyphs with the palette observed at their draw boundary.

    The selected eight-bit texture palette is independent of the font's BPAL.
    The blend unit reads stored RGB565 with zero low bits, subtracts dithering,
    floors source and destination terms independently, then stores RGB565 again.
    `right` is the observed exclusive raster clip, not a comparison mask.
    """
    if len(palette) != 256 or not 0 < right <= image.width:
        raise ValueError('subtitle draw requires a complete selected palette and clip')
    spec = importlib.util.spec_from_file_location('presentation', Path(__file__).with_name('cinematic-presented-frames.py'))
    presentation = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(presentation)
    pixels = np.array(image.convert('RGB'), dtype=np.int64)
    shifts = np.array([3, 2, 3])
    scale = np.array([1, 2, 1])
    for x, y, index, shadow, alpha in presentation.subtitle_pixels(metadata, frame, enabled):
        if 0 <= x < right and 0 <= y < image.height:
            source = np.array([0, 0, 0] if shadow else palette[index])
            destination = (pixels[y, x] >> shifts) << shifts
            dither = DITHER[y % 4, x % 4]
            destination = ((destination << scale) + 15 - dither) >> scale
            colour = (source * (alpha + 1) >> 8) + (destination * (256 - alpha) >> 8)
            pixels[y, x] = store565(colour.reshape(1, 1, 3), x, y)[0, 0]
    return Image.fromarray(pixels.astype(np.uint8), 'RGB').convert('RGBA')


def require_equal(actual, expected):
    """Reject every changed pixel, including pixels beyond a decoded prefix."""
    if actual.size != expected.size:
        raise ValueError('presentation dimensions differ')
    a, b = np.asarray(actual.convert('RGBA')), np.asarray(expected.convert('RGBA'))
    count = int(np.any(a != b, axis=2).sum())
    if count:
        raise ValueError(f'full raster differs at {count} pixels')
    return {'pixels': actual.width * actual.height,
            'sha256': hashlib.sha256(a.tobytes()).hexdigest()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('decoded', type=Path)
    parser.add_argument('state', type=Path)
    parser.add_argument('presentation', type=Path)
    parser.add_argument('--output', type=Path)
    parser.add_argument('--subtitles', type=Path, help='installed movie metadata; requires frame/palette/clip in state')
    args = parser.parse_args()
    state = json.loads(args.state.read_text())
    expected = compose(Image.open(args.decoded), state['padding_colour'], state['draws'])
    if args.subtitles:
        expected = compose_subtitles(expected, json.loads(args.subtitles.read_text()),
                                     state['frame'], state['subtitle_palette'],
                                     state['subtitles_enabled'], state['subtitle_right'])
    if args.output:
        expected.save(args.output)
    print(json.dumps(require_equal(Image.open(args.presentation), expected), indent=2))


if __name__ == '__main__':
    main()
