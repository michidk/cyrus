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


def read_native_dispatch(read, functions, *, table, count, handler):
    """Identify a stopped handler-table selection without a bytecode call record.

    The observed count must match the installed definition, including its null
    entry. This supports native hooks as well as encoded calls. A handler write
    precedes invocation; it does not prove that the handler has returned or is
    still active at a later event. Shared handler addresses are ambiguous and
    reject rather than borrowing a name from a neighbouring dispatch.
    """
    if (type(count) is not int or count < 2 or count != len(functions)
            or type(table) is not int or table <= 0
            or type(handler) is not int or handler <= 0):
        raise ValueError('native dispatch table/count/handler binding unavailable or inconsistent')
    data = read(table, count * 4)
    if len(data) != count * 4:
        raise ValueError('native dispatch handler table truncated')
    matches = [index for index, target in enumerate(struct.unpack(f'<{count}I', data))
               if target == handler]
    if len(matches) != 1 or matches[0] == 0:
        raise ValueError('native dispatch handler unknown, null or ambiguous')
    index = matches[0]
    name = functions[index].get('name')
    if not isinstance(name, str) or not name:
        raise ValueError('native dispatch installed function name unavailable')
    return dict(function=name, function_id=index)
