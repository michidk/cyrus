#!/usr/bin/env python3
"""The live adapter regression must select an original slot, including shared numbers."""
import importlib.util
import unittest
from pathlib import Path

path = Path(__file__).with_name("verify-original-slot.py")
spec = importlib.util.spec_from_file_location("verify_original_slot", path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class SlotSelectionTest(unittest.TestCase):
    def menu(self):
        return {
            "slots": [
                {"slot": 2, "source": {"type": "native"}},
                {"slot": 2, "source": {"type": "original", "world_id": 20}},
            ],
            "entries": [{"action": 0}, {"action": 202}, {"action": 202}],
        }

    def test_original_identity_wins_over_same_native_number(self):
        self.assertEqual(module.slot_row(self.menu(), 2, 20), 2)

    def test_changed_world_or_duplicate_original_rejects(self):
        with self.assertRaises(AssertionError):
            module.slot_row(self.menu(), 2, 1)
        menu = self.menu()
        menu["slots"].append(menu["slots"][1])
        with self.assertRaises(AssertionError):
            module.slot_row(menu, 2, 20)


if __name__ == "__main__":
    unittest.main()
