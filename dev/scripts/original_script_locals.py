#!/usr/bin/env python3
"""Observe original script locals with installed allocation metadata.

Executable bindings stay in the local reader. The caller supplies its heap-read
function, live variable-pool base and instance byte offset, and the installed
script's authored variable count. RAVA instance allocation is documented in
preservation's RGM format and SOUP engine references.
"""
import struct


def read_locals(read_heap, pool, offset, count, context):
    """Read every signed local; absent metadata or short reads reject observation.

    A missing allocation is not an empty allocation. Only an authored zero count
    establishes empty locals without reading memory. This function never derives
    a count from a local's value or a native restored checkpoint.
    """
    if type(count) is not int or count < 0:
        raise ValueError(f'{context}: authored local count unavailable or invalid')
    if count == 0:
        return []
    if type(pool) is not int or pool <= 0 or type(offset) is not int or offset <= 0:
        raise ValueError(f'{context}: local allocation binding unavailable or invalid')
    data = read_heap(pool + offset, count * 4)
    if len(data) != count * 4:
        raise ValueError(f'{context}: expected {count * 4} local bytes, observed {len(data)}')
    return list(struct.unpack(f'<{count}i', data))
