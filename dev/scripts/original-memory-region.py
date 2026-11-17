#!/usr/bin/env python3
"""Locate a bounded original-runtime region in anonymous process mappings.

Executable-specific signatures and offsets belong to the local reader. Offsets
and sizes passed here are bytes; returned addresses are process virtual addresses.
"""


def locate(memory, ranges, signature, anchor_offset, region_size):
    """Require one signature-anchored region wholly contained in a readable mapping."""
    if not signature or anchor_offset < 0 or region_size < anchor_offset + len(signature):
        raise ValueError('original memory region: invalid signature bounds')
    candidates = set()
    for start, end in ranges:
        if end - start < region_size:
            continue
        # A coalesced anonymous mapping can include far more than guest RAM.
        # Search only possible anchor positions, retaining overlap so signatures
        # spanning a read boundary are still found without copying the mapping.
        position = start + anchor_offset
        limit = end - region_size + anchor_offset + len(signature)
        overlap = b''
        while position < limit:
            try:
                memory.seek(position)
                chunk = memory.read(min(65536, limit - position))
            except OSError:
                # A process mapping can disappear after its maps snapshot.
                break
            if not chunk:
                break
            blob = overlap + chunk
            origin = position - len(overlap)
            match = blob.find(signature)
            while match != -1:
                base = origin + match - anchor_offset
                if base >= start and base + region_size <= end:
                    candidates.add(base)
                match = blob.find(signature, match + 1)
            position += len(chunk)
            overlap = blob[-(len(signature) - 1):] if len(signature) > 1 else b''
    if len(candidates) != 1:
        raise ValueError(f'original memory region: expected one binding, found {len(candidates)}')
    return candidates.pop()
