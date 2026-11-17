#!/usr/bin/env python3
"""Check a terrain-only crop from the public screenshot command.

Usage: test-terrain-framebuffer.py SCREENSHOT LEFT TOP RIGHT BOTTOM
The caller must select unobscured Faithful terrain, excluding HUD and models.
"""
import sys
from PIL import Image


def check(image):
    pixels = list(image.convert('RGB').getdata())
    if not pixels:
        raise AssertionError('terrain crop is empty')
    for channel, bits in enumerate((5, 6, 5)):
        allowed = {(value << (8 - bits)) | (value >> (2 * bits - 8))
                   for value in range(1 << bits)}
        invalid = sum(pixel[channel] not in allowed for pixel in pixels)
        if invalid:
            raise AssertionError(f'terrain channel {channel}: {invalid}/{len(pixels)} pixels exceed framebuffer precision')
    if len(set(pixels)) < 16:
        raise AssertionError('terrain crop lacks texture variation')


if __name__ == '__main__':
    with Image.open(sys.argv[1]) as image:
        crop = image.crop(tuple(map(int, sys.argv[2:6])))
        check(crop)
        print(f'Faithful terrain framebuffer precision: {crop.width * crop.height} pixels accepted')
