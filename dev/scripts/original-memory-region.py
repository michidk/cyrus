#!/usr/bin/env python3
"""Locate a bounded original-runtime region in anonymous process mappings.

Executable-specific signatures and offsets belong to the local reader. Offsets
and sizes passed here are bytes; returned addresses are process virtual addresses.
"""


def locate(memory, ranges, signature, anchor_offset, region_size):
    """Require one signature-anchored region wholly contained in a mapping."""
    if not signature or anchor_offset < 0 or region_size < anchor_offset + len(signature):
        raise ValueError('original memory region: invalid signature bounds')
    candidates = set()
    for start, end in ranges:
        if end - start < region_size:
            continue
        memory.seek(start)
        blob = memory.read(end - start)
        match = blob.find(signature, anchor_offset)
        while match != -1:
            base = start + match - anchor_offset
            if base + region_size <= end:
                candidates.add(base)
            match = blob.find(signature, match + 1)
    if len(candidates) != 1:
        raise ValueError(f'original memory region: expected one binding, found {len(candidates)}')
    return candidates.pop()
