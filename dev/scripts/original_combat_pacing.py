"""Read original combat decision pacing from independently established bindings."""
import struct


def read_combat_pacing(read, bindings):
    """Retain the cached decision clock separately from update and tick counters.

    Callers establish the receiver, decision lifetime and actual tracee access.
    A stopped endpoint alone does not prove a picker ran. Signed wrapping sums
    and truncating remainders follow preservation's nonplayer selection rules.
    No process pointers or executable bindings enter the returned projection.
    """
    result = {}
    formats = dict(cached_bios_sample='i', distance_q8='i', allocation_index='i',
                   action='i', defence_counter='i', attack_pacing='i',
                   defence_pacing='i', random_state='I')
    for name, format in formats.items():
        address = bindings.get(name)
        if type(address) is not int or address < 0:
            raise ValueError(f'combat pacing {name}: binding unavailable')
        data = read(address, 4)
        if len(data) != 4:
            raise ValueError(f'combat pacing {name}: expected four bytes, observed {len(data)}')
        result[name] = struct.unpack('<' + format, data)[0]
    total = (result['cached_bios_sample'] + result['allocation_index']) & ((1 << 32) - 1)
    result['pacing_sum'] = total if total < (1 << 31) else total - (1 << 32)
    result['remainders'] = {}
    for name in ('defence_counter', 'attack_pacing', 'defence_pacing'):
        divisor = result[name]
        if divisor == 0 or (result['pacing_sum'] == -(1 << 31) and divisor == -1):
            raise ValueError(f'combat pacing {name}: undefined signed remainder')
        quotient = abs(result['pacing_sum']) // abs(divisor)
        if (result['pacing_sum'] < 0) != (divisor < 0):
            quotient = -quotient
        result['remainders'][name] = result['pacing_sum'] - quotient * divisor
    return result
