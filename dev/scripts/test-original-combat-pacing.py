"""Pacing observations reject absent fields and distinguish clock phase/units."""
import struct
import unittest
from original_combat_pacing import read_combat_pacing


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


if __name__ == '__main__': unittest.main()
