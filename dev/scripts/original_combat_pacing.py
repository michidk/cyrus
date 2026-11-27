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


class CombatTimingEvents:
    """Order stopped observations without inferring a decision from an endpoint.

    The local observer establishes actor and field bindings. Only an actual
    tracee read of a pacing field may publish a combat-read witness. Cache and
    baseline are unsigned BIOS ticks; body numbers count completed updates.
    """
    FIELDS = ('defence_counter', 'attack_pacing', 'defence_pacing')

    def __init__(self, emit):
        self.emit = emit
        self.sequence = 0
        self.body = 0

    def observe(self, kind, body, clock, *, actor=None, field=None, access=None, decision_sample=None):
        if type(body) is not int or not self.body <= body <= self.body + 1:
            raise ValueError('combat timing body must be contiguous and nondecreasing')
        if kind not in ('baseline-write', 'body-boundary', 'input-ack', 'combat-read', 'decision-cache-read', 'decision-cache-write'):
            raise ValueError('unsupported combat timing event')
        required = ('sample', 'baseline', 'frames', 'remainder')
        if not isinstance(clock, dict) or any(type(clock.get(k)) is not int or not 0 <= clock[k] < 2**32 for k in required):
            raise ValueError('combat timing requires independently observed unsigned clock fields')
        if kind == 'combat-read' and (not isinstance(actor, str) or not actor or field not in self.FIELDS or access != 'read'):
            raise ValueError('combat timing requires actor and actual pacing-field read witness')
        if kind == 'combat-read' and (type(decision_sample) is not int or not 0 <= decision_sample < 2**32):
            raise ValueError('combat read requires its independently observed decision sample')
        if kind.startswith('decision-cache-'):
            if type(decision_sample) is not int or not 0 <= decision_sample < 2**32 or access != kind.removeprefix('decision-cache-'):
                raise ValueError('decision cache requires independent sample and matching tracee access')
            if access == 'write' and decision_sample != clock['sample']:
                raise ValueError('decision entry cache differs from observed assignment source')
        row = dict(sequence=self.sequence, body=body, kind=kind, clock=dict(clock))
        if kind == 'combat-read':
            row.update(actor=actor, field=field, access=access, decision_sample=decision_sample)
        if kind.startswith('decision-cache-'):
            row.update(decision_sample=decision_sample, access=access)
        self.emit(row)
        self.sequence += 1
        self.body = body
        return row


def combat_read_sample(event, *, actor, field):
    """Project only the identified tracee read, never a later boundary sample."""
    if (not isinstance(event, dict) or event.get('kind') != 'combat-read'
            or event.get('access') != 'read' or event.get('actor') != actor
            or event.get('field') != field or field not in CombatTimingEvents.FIELDS):
        raise ValueError('combat sample requires the identified actor pacing read')
    value = event.get('decision_sample')
    if type(value) is not int or not 0 <= value < 2**32:
        raise ValueError('combat sample lacks independent decision sample')
    return value


class DecisionCacheHistory:
    """Bind cache consumers to the preceding witnessed receiver invocation.

    Receiver identity is resolved from the original call frame by the local
    binding, never from registry order or a later body snapshot. Occurrences
    count entries for that receiver within one ordinary body.
    """
    def __init__(self):
        self.current = None
        self.occurrences = {}
        self.last_sequence = -1

    def observe(self, event, *, receiver=None):
        if not isinstance(event, dict) or event.get('kind') not in ('decision-cache-read', 'decision-cache-write'):
            raise ValueError('decision history requires an actual cache access')
        if event.get('access') != event['kind'].removeprefix('decision-cache-'):
            raise ValueError('decision history lacks matching tracee access witness')
        sequence = event.get('sequence')
        if type(sequence) is not int or sequence <= self.last_sequence:
            raise ValueError('decision history requires strictly ordered access sequence')
        sample, body = event.get('decision_sample'), event.get('body')
        if type(sample) is not int or not 0 <= sample < 2**32 or type(body) is not int or body < 1:
            raise ValueError('decision history requires body and independent unsigned sample')
        if event['kind'] == 'decision-cache-write':
            if not isinstance(receiver, str) or not receiver:
                raise ValueError('decision entry requires independently resolved receiver')
            if self.current is not None and body < self.current['body']:
                raise ValueError('decision history cannot reorder bodies')
            if self.current is None or body != self.current['body']:
                self.occurrences.clear()
            occurrence = self.occurrences.get(receiver, 0) + 1
            self.occurrences[receiver] = occurrence
            self.current = dict(body=body, receiver=receiver, occurrence=occurrence, decision_sample=sample)
        elif (self.current is None or body != self.current['body']
              or sample != self.current['decision_sample']
              or (receiver is not None and receiver != self.current['receiver'])):
            raise ValueError('decision read lacks matching current receiver/cache assignment')
        self.last_sequence = sequence
        return dict(event, **self.current)


def measurement_from_baseline(previous_clock, event):
    """Validate a calculation's baseline store against retained pre-body state.

    The baseline-store witness follows the remainder addition and precedes
    publication. A refresh with unchanged measurement count is not a calculation.
    Both the stored sample gap and observed remainder addition must agree.
    """
    from original_acquisition import next_bios_input
    current = event.get('clock', {})
    if (event.get('kind') != 'baseline-write'
            or current.get('frames') == previous_clock.get('frames')
            or current.get('baseline') != current.get('sample')):
        raise ValueError('measurement requires the calculation baseline-store witness')
    sample = current.get('sample')
    row = dict(bios_ticks=sample, clock_publication=dict(bios_sample=sample),
               bios_measurement=dict(phase='before_publication', sample=sample,
                   baseline=previous_clock.get('baseline'),
                   remainder_before=previous_clock.get('remainder'),
                   remainder_after=current.get('remainder')))
    return dict(stored_sample=sample, effective_elapsed_ticks=next_bios_input(row))
