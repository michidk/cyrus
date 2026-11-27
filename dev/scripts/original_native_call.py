#!/usr/bin/env python3
"""Decode stopped native-call identity from locally verified original bindings."""
import struct


def read_native_call(read, functions, *, call, table, receiver, selected_source, handler):
    """Resolve an encoded call and matching dispatch handler to its installed name.

    Call records carry a little-endian function index and authored argument count;
    the runtime table contains four-byte handler pointers. Layout rules are in
    preservation's SOUP native-dispatch reference. The caller binds these fields
    at the same stopped event. A matching selected source is reported separately
    from the receiver. This snapshot cannot establish an active call lifetime;
    callers must observe dispatch/return ordering before asserting causality.
    Addresses stay in the local reader, outside this semantic projection.
    """
    for name, value in dict(call=call, table=table, receiver=receiver,
                            selected_source=selected_source, handler=handler).items():
        if type(value) is not int or value <= 0:
            raise ValueError(f'native call {name} binding unavailable')
    def exact(address, size):
        data = read(address, size)
        if len(data) != size:
            raise ValueError(f'native call expected {size} bytes, observed {len(data)}')
        return data
    index, count = struct.unpack('<HB', exact(call, 3))
    if index == 0 or index >= len(functions) or count > 8:
        raise ValueError('native call index or argument count outside supported dispatch')
    name = functions[index].get('name')
    if not isinstance(name, str) or not name:
        raise ValueError('native call installed function name unavailable')
    target = struct.unpack('<I', exact(table + index * 4, 4))[0]
    if target != handler:
        raise ValueError('native call handler differs from installed runtime dispatch table')
    return dict(function=name, function_id=index, argument_count=count,
                receiver_is_selected_source=receiver == selected_source)
