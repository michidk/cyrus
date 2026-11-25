#!/usr/bin/env python3
"""Check a world surface crop from the public screenshot command.

Usage: test-world-framebuffer.py SCREENSHOT LEFT TOP RIGHT BOTTOM
Regression: test-world-framebuffer.py --regression ORIGINAL BEFORE AFTER LEFT TOP RIGHT BOTTOM
The caller must select unobscured Faithful sky, terrain, models or placed flats, excluding HUD.
"""
import sys
if __name__ == '__main__' and len(sys.argv) == 1:
    print('skipped: requires an installed-game framebuffer capture')
    raise SystemExit(0)

import sys
from PIL import Image


class FramebufferPrecisionError(AssertionError):
    """A rendered channel cannot be represented by the framebuffer."""


def check(image):
    pixels = list(image.convert('RGB').getdata())
    if not pixels:
        raise AssertionError('world surface crop is empty')
    for channel, bits in enumerate((5, 6, 5)):
        allowed = {(value << (8 - bits)) | (value >> (2 * bits - 8))
                   for value in range(1 << bits)}
        invalid = sum(pixel[channel] not in allowed for pixel in pixels)
        if invalid:
            raise FramebufferPrecisionError(f'world surface channel {channel}: {invalid}/{len(pixels)} pixels exceed framebuffer precision')
    if len(set(pixels)) < 16:
        raise AssertionError('world surface crop lacks texture variation')


def regression(reference, before, after, bounds):
    if reference.size != before.size or reference.size != after.size:
        raise AssertionError('matched framebuffer screenshots have different dimensions')
    left, top, right, bottom = bounds
    if not (0 <= left < right <= reference.width and 0 <= top < bottom <= reference.height):
        raise AssertionError('framebuffer crop is empty or outside the image')
    check(reference.crop(bounds))
    check(after.crop(bounds))
    try:
        check(before.crop(bounds))
    except FramebufferPrecisionError:
        pass
    else:
        raise AssertionError('baseline already satisfies framebuffer precision')
    print(f'Faithful framebuffer regression: {(right-left)*(bottom-top)} pixels accepted')


if __name__ == '__main__':
    if sys.argv[1] == '--regression':
        images = [Image.open(path) for path in sys.argv[2:5]]
        try:
            regression(*images, tuple(map(int, sys.argv[5:9])))
        finally:
            for image in images:
                image.close()
    else:
        with Image.open(sys.argv[1]) as image:
            crop = image.crop(tuple(map(int, sys.argv[2:6])))
            check(crop)
            print(f'Faithful world framebuffer precision: {crop.width * crop.height} pixels accepted')
