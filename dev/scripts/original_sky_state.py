#!/usr/bin/env python3
"""Decode stopped original sky continuation using locally established bindings."""
import math
import struct


def read_sky_state(read, bindings):
    """Read sky activation, direction, speed and BIOS scrolling continuation.

    Direction uses 2048 engine angle steps; speed and offsets are texture texels,
    with speed per BIOS tick. The texture handle is an opaque lifetime token,
    not an asset identity. Compare it only within the same process. The caller
    must stop execution and independently bind the selected installed sky asset.
    Layout and scrolling rules are documented in preservation's sky reference.
    This snapshot cannot establish that no intervening reroll occurred; observe
    direction writes across the interval for that claim.
    """
    formats = dict(enabled='<I', direction='<I', speed='<f', offset_u='<f',
                   offset_v='<f', last_tick='<I', texture_handle='<i')
    result = {}
    for field, fmt in formats.items():
        address = bindings.get(field)
        if type(address) is not int or address <= 0:
            raise ValueError(f'sky {field}: binding unavailable')
        data = read(address, 4)
        if len(data) != 4:
            raise ValueError(f'sky {field}: expected four bytes, observed {len(data)}')
        result[field] = struct.unpack(fmt, data)[0]
    if result['enabled'] not in (0, 1):
        raise ValueError('sky enabled: unsupported activation value')
    if result['direction'] >= 2048:
        raise ValueError('sky direction: outside engine angle range')
    for field in ('speed', 'offset_u', 'offset_v'):
        if not math.isfinite(result[field]):
            raise ValueError(f'sky {field}: nonfinite continuation')
    result['enabled'] = bool(result['enabled'])
    return result
