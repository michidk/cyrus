"""Pacing observations reject absent fields and distinguish clock phase/units."""
import struct
import unittest
from original_combat_pacing import read_combat_pacing, CombatTimingEvents, combat_read_sample, DecisionCacheHistory, measurement_from_baseline


class CombatPacingTests(unittest.TestCase):
    def setUp(self):
        self.values = dict(cached_bios_sample=101, distance_q8=1234,
                           allocation_index=3, action=-1, defence_counter=13,
                           attack_pacing=2, defence_pacing=1, random_state=4275878552)
        self.bindings = {name: index * 4 for index, name in enumerate(self.values)}

    def observe(self):
        data = b''.join(struct.pack('<I', value & ((1 << 32) - 1)) for value in self.values.values())
        return read_combat_pacing(lambda address, size: data[address:address+size], self.bindings)

    def test_exact_signed_state_and_paced_boundary(self):
        result = self.observe()
        self.assertEqual(result['pacing_sum'], 104)
        self.assertEqual(result['remainders'], dict(defence_counter=0, attack_pacing=0, defence_pacing=0))
        self.assertEqual(result['action'], -1)
        self.assertEqual(result['random_state'], 4275878552)

    def test_next_sample_or_effective_interval_is_not_the_cached_clock(self):
        for substitute in (102, 1):
            self.values['cached_bios_sample'] = substitute
            self.assertNotEqual(self.observe()['remainders']['defence_counter'], 0)

    def test_wrapping_sum_and_truncating_signed_remainder(self):
        self.values.update(cached_bios_sample=((1 << 31) - 1), allocation_index=2, defence_counter=3)
        result = self.observe()
        self.assertEqual(result['pacing_sum'], -2147483647)
        self.assertEqual(result['remainders']['defence_counter'], -1)

    def test_missing_truncated_and_undefined_reject(self):
        for name in self.bindings:
            bindings = dict(self.bindings); del bindings[name]
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'binding'):
                read_combat_pacing(lambda address, size: b'\0' * size, bindings)
        with self.assertRaisesRegex(ValueError, 'expected four'):
            read_combat_pacing(lambda address, size: b'\0', self.bindings)
        for name in ('defence_counter', 'attack_pacing', 'defence_pacing'):
            previous = self.values[name]; self.values[name] = 0
            with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'undefined'):
                self.observe()
            self.values[name] = previous
        self.values.update(cached_bios_sample=-(1 << 31), allocation_index=0, defence_counter=-1)
        with self.assertRaisesRegex(ValueError, 'undefined'): self.observe()


class TimingEventsTests(unittest.TestCase):
    def test_read_retains_earlier_sample_separately_from_later_boundary(self):
        rows = []; events = CombatTimingEvents(rows.append)
        clock = dict(sample=100, baseline=100, frames=4, remainder=2)
        events.observe('combat-read', 1, clock, actor='opponent', field='defence_counter', access='read', decision_sample=99)
        clock.update(sample=145, baseline=145)
        events.observe('baseline-write', 1, clock)
        events.observe('body-boundary', 1, clock)
        self.assertEqual([r['sequence'] for r in rows], [0, 1, 2])
        self.assertEqual(rows[0]['clock']['sample'], 100)
        self.assertEqual(rows[-1]['clock']['sample'], 145)
        self.assertEqual(combat_read_sample(rows[0], actor='opponent', field='defence_counter'), 99)
        for event in [rows[-1], dict(rows[0], actor='other'), dict(rows[0], decision_sample=None)]:
            with self.assertRaises(ValueError):
                combat_read_sample(event, actor='opponent', field='defence_counter')

    def test_cache_access_requires_own_witness_and_retains_old_decision_sample(self):
        rows = []; events = CombatTimingEvents(rows.append)
        clock = dict(sample=100, baseline=100, frames=4, remainder=2)
        events.observe('decision-cache-write', 1, clock, access='write', decision_sample=100)
        clock.update(sample=145, baseline=145)
        events.observe('decision-cache-read', 1, clock, access='read', decision_sample=100)
        self.assertEqual(rows[-1]['decision_sample'], 100)
        for kwargs in [dict(), dict(access='write', decision_sample=145)]:
            with self.assertRaisesRegex(ValueError, 'matching tracee access'):
                events.observe('decision-cache-read', 1, clock, **kwargs)
        with self.assertRaisesRegex(ValueError, 'assignment source'):
            events.observe('decision-cache-write', 1, clock, access='write', decision_sample=100)

    def test_endpoint_or_write_cannot_masquerade_as_read(self):
        events = CombatTimingEvents(lambda row: None)
        clock = dict(sample=100, baseline=100, frames=4, remainder=2)
        for fields in [dict(), dict(actor='opponent', field='body-boundary', access='read'),
                       dict(actor='opponent', field='defence_counter', access='write')]:
            with self.subTest(fields=fields), self.assertRaisesRegex(ValueError, 'read witness'):
                events.observe('combat-read', 1, clock, **fields)
        events.observe('body-boundary', 1, clock)
        with self.assertRaisesRegex(ValueError, 'contiguous'):
            events.observe('body-boundary', 3, clock)
        events.observe('body-boundary', 2, clock)
        with self.assertRaisesRegex(ValueError, 'nondecreasing'):
            events.observe('baseline-write', 1, clock)
        del clock['baseline']
        with self.assertRaisesRegex(ValueError, 'independently observed'):
            events.observe('baseline-write', 2, clock)


class DecisionHistoryTests(unittest.TestCase):
    def test_receiver_occurrence_and_read_sample_survive_later_endpoint(self):
        history = DecisionCacheHistory()
        entry = dict(kind='decision-cache-write', access='write', sequence=0, body=1, decision_sample=100)
        read = dict(kind='decision-cache-read', access='read', sequence=1, body=1, decision_sample=100,
                    clock=dict(sample=145, baseline=145))
        self.assertEqual(history.observe(entry, receiver='opponent')['occurrence'], 1)
        actual = history.observe(read)
        self.assertEqual((actual['receiver'], actual['decision_sample']), ('opponent', 100))
        self.assertEqual(history.observe(dict(entry, sequence=2), receiver='opponent')['occurrence'], 2)
        self.assertEqual(history.observe(dict(entry, body=2, sequence=3), receiver='opponent')['occurrence'], 1)

    def test_missing_assignment_wrong_receiver_and_boundary_sample_reject(self):
        history = DecisionCacheHistory()
        entry = dict(kind='decision-cache-write', access='write', sequence=0, body=1, decision_sample=100)
        read = dict(kind='decision-cache-read', access='read', sequence=1, body=1, decision_sample=100)
        with self.assertRaises(ValueError): history.observe(read)
        with self.assertRaises(ValueError): history.observe(entry)
        history.observe(entry, receiver='opponent')
        for event, receiver in [(dict(read, decision_sample=145), None),
                                (read, 'other'), (dict(read, sequence=0), None), (dict(read, access=None), None), (dict(read, body=2), None),
                                (dict(read, kind='body-boundary'), None)]:
            with self.subTest(event=event), self.assertRaises(ValueError):
                history.observe(event, receiver=receiver)


class MeasurementWitnessTests(unittest.TestCase):
    def test_independent_addition_agrees_with_sample_and_retained_baseline(self):
        previous = dict(sample=143, baseline=143, frames=4, remainder=3)
        event = dict(kind='baseline-write', clock=dict(sample=145, baseline=145, frames=5, remainder=5))
        self.assertEqual(measurement_from_baseline(previous, event),
                         dict(stored_sample=145, effective_elapsed_ticks=2))
        for clock in [dict(event['clock'], frames=4), dict(event['clock'], remainder=6),
                      dict(event['clock'], remainder=1), dict(event['clock'], baseline=143)]:
            with self.subTest(clock=clock), self.assertRaises(ValueError):
                measurement_from_baseline(previous, dict(event, clock=clock))
        with self.assertRaises(ValueError):
            measurement_from_baseline(previous, dict(event, kind='body-boundary'))


if __name__ == '__main__': unittest.main()
