#!/usr/bin/env python3
"""Check a world surface crop from the public screenshot command.

Usage: test-world-framebuffer.py SCREENSHOT LEFT TOP RIGHT BOTTOM
The caller must select unobscured Faithful terrain, models or placed flats, excluding HUD.
"""
import sys
from PIL import Image


def check(image):
    pixels = list(image.convert('RGB').getdata())
    if not pixels:
        raise AssertionError('world surface crop is empty')
    for channel, bits in enumerate((5, 6, 5)):
        allowed = {(value << (8 - bits)) | (value >> (2 * bits - 8))
                   for value in range(1 << bits)}
        invalid = sum(pixel[channel] not in allowed for pixel in pixels)
        if invalid:
            raise AssertionError(f'world surface channel {channel}: {invalid}/{len(pixels)} pixels exceed framebuffer precision')
    if len(set(pixels)) < 16:
        raise AssertionError('world surface crop lacks texture variation')


if __name__ == '__main__':
    with Image.open(sys.argv[1]) as image:
        crop = image.crop(tuple(map(int, sys.argv[2:6])))
        check(crop)
        print(f'Faithful world framebuffer precision: {crop.width * crop.height} pixels accepted')
