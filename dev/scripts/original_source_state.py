#!/usr/bin/env python3
"""Observe decoded native-source state using locally evidenced memory bindings.

No executable layout is distributed here. The caller binds the stopped actor's
fields and current variable/string pools. Preservation's SOUP parameter rules
select variables from the receiver and strings from the retained source, using
current process pools plus the corresponding actor's instance byte offsets.
"""
import struct
from original_script_locals import read_locals


def read_source_state(read_heap, bindings, script, attribute_names):
    """Read complete source attributes, stored orientation, locals and strings.

    Orientation retains signed angle words and row-major IEEE matrix bits without
    refreshing from angles. Attribute names come from the installed definitions;
    unnamed slots remain indexed data. Strings are observed through the current
    relocated pointer table, not substituted from installed text or old captures.
    Missing bindings, authored counts, truncated reads and unterminated strings
    reject this observation. The caller supplies a stopped, identity-bound source.
    """
    context = script.get('name')
    def exact(pointer, size):
        if type(pointer) is not int or pointer <= 0:
            raise ValueError(f'{context}: native source memory binding unavailable')
        data = read_heap(pointer, size)
        if len(data) != size:
            raise ValueError(f'{context}: expected {size} source bytes, observed {len(data)}')
        return data
    def address(name):
        value = bindings.get(name)
        if type(value) is not int or value <= 0:
            raise ValueError(f'{context}: missing source {name} binding')
        return value
    def words(name, count, signed=False):
        return list(struct.unpack('<' + str(count) + ('i' if signed else 'I'),
                                  exact(address(name), count * 4)))
    if (not isinstance(context, str) or not context
            or any(type(index) is not int or not 0 <= index < 256
                   or not isinstance(name, str) or not name for index, name in attribute_names.items())
            or len(set(attribute_names.values())) != len(attribute_names)):
        raise ValueError('native source requires unique installed attribute names and script identity')
    attributes = exact(address('attributes'), 256)
    matrix = words('matrix', 9)
    variables = read_locals(exact, bindings.get('local_pool'), bindings.get('local_offset'),
                            script.get('local_count'), context)
    count = script.get('string_count')
    if type(count) is not int or not 0 <= count <= 256:
        raise ValueError(f'{context}: authored source string count unavailable or invalid')
    strings = []
    if count:
        offset = bindings.get('string_offset')
        if type(offset) is not int or offset < 0 or offset % 4:
            raise ValueError(f'{context}: source string instance offset unavailable or invalid')
        pointers = struct.unpack('<' + str(count) + 'I', exact(address('string_pool') + offset, count * 4))
        for index, pointer in enumerate(pointers):
            if not pointer:
                raise ValueError(f'{context}: source string {index} pointer unavailable')
            value = bytearray()
            for byte in range(4096):
                char = exact(pointer + byte if pointer else 0, 1)
                if char == b'\0':
                    strings.append(value.decode('latin-1'))
                    break
                value.extend(char)
            else:
                raise ValueError(f'{context}: source string {index} exceeds observation bound')
    return dict(attributes=dict(named={name: attributes[index] for index, name in attribute_names.items()},
                                unnamed={str(index): value for index, value in enumerate(attributes)
                                         if index not in attribute_names}),
                position_q8=words('position_q8', 3, True),
                orientation=dict(angles=words('angles', 3, True),
                                 matrix=[matrix[i:i + 3] for i in range(0, 9, 3)],
                                 model_position=words('model_position', 3)),
                script_context=dict(name=context, variables=variables, strings=strings))
