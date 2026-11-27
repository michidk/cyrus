#!/usr/bin/env python3
"""Read original loading-artwork policy without inferring a new selection."""
import struct


def read_loading_state(read, bindings):
    """Read the artwork-enable flag and retained zero-based frame index.

    Bindings are established locally at a stopped boundary. The index persists
    when artwork is disabled and does not identify a newly selected image.
    Observe policy/index/RNG writes around the load, and bind the loading route
    independently, before attributing a random draw to selection. Book Load's
    saved-screen route and ordinary artwork rules are documented in preservation.
    """
    result = {}
    for field in ('artwork_enabled', 'retained_frame'):
        address = bindings.get(field)
        if type(address) is not int or address <= 0:
            raise ValueError(f'loading {field}: binding unavailable')
        data = read(address, 4)
        if len(data) != 4:
            raise ValueError(f'loading {field}: expected four bytes, observed {len(data)}')
        result[field] = struct.unpack('<I', data)[0]
    if result['artwork_enabled'] not in (0, 1):
        raise ValueError('loading artwork_enabled: unsupported policy value')
    result['artwork_enabled'] = bool(result['artwork_enabled'])
    return result
