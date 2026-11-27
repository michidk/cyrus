"""Read a completed original support result from locally bound semantic fields."""
import struct


def read_support_state(read, bindings):
    """Retain contact validity separately from stale contact metadata after a miss.

    Bindings address the completed support record's fields, not an actor surface
    byte. Callers must establish query ownership and completion independently.
    Heights use Q8 map units; normals use the engine's integer normal scale.
    Source kinds, miss behavior and classification are documented in
    preservation's player ground-support rules. Process pointers are omitted.
    """
    def value(name, format):
        address = bindings.get(name)
        if type(address) is not int or address < 0:
            raise ValueError(f'support {name} binding unavailable')
        size = struct.calcsize(format)
        data = read(address, size)
        if len(data) != size:
            raise ValueError(f'support {name} expected {size} bytes, observed {len(data)}')
        result = struct.unpack(format, data)
        return result[0] if len(result) == 1 else list(result)

    kind = value('source_kind', '<B')
    classification = value('classification', '<B')
    height = value('actor_height', '<i')
    kinds = {0: 'none', 0x20: 'model', 0x40: 'terrain'}
    if kind not in kinds:
        raise ValueError(f'support source kind {kind} unsupported')
    present = kind != 0
    if (not present and (height != -(1 << 27) or classification != 0)
            or present and (height == -(1 << 27) or classification not in (1, 2, 4, 8))):
        raise ValueError('support result incomplete or inconsistent with contact validity')
    return dict(source=kinds[kind], present=present, classification=classification,
                actor_height_q8=height, normal=value('normal', '<3i'),
                texture=value('texture', '<H'), metadata=value('metadata', '<B'))
