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

    def test_loader_failure_rejects_before_waiting_for_admission(self):
        class FailedLoad:
            def request(self, _command):
                return {"menu": {"unavailable": "original save slot fixture: expected a directory"}}

        with self.assertRaisesRegex(AssertionError, "expected a directory"):
            module.wait_state(FailedLoad(), lambda _: False, 60)

    def test_changed_world_or_duplicate_original_rejects(self):
        with self.assertRaises(AssertionError):
            module.slot_row(self.menu(), 2, 1)
        menu = self.menu()
        menu["slots"].append(menu["slots"][1])
        with self.assertRaises(AssertionError):
            module.slot_row(menu, 2, 20)


class ScriptCensusTest(unittest.TestCase):
    class Inspector:
        def __init__(self, replace=False):
            self.calls=[]
            self.replace=replace
        def request(self, command):
            self.calls.append(command)
            start=command.get('offset',0)
            return {'total':130,'world':1,'generation':2 if self.replace and start else 1,
                    'content_id':'installed','actors':[{'id':i,'selected':i==129}
                    for i in range(start,min(start+64,130))]}

    def test_selected_owner_after_the_first_page_is_retained(self):
        control=self.Inspector()
        result=module.inspect_scripts(control)
        self.assertEqual(len(result['actors']),130)
        self.assertEqual([a['id'] for a in result['actors'] if a['selected']],[129])
        self.assertEqual([c['offset'] for c in control.calls],[0,64,128])

    def test_replacement_during_inspection_rejects_mixed_owners(self):
        with self.assertRaisesRegex(AssertionError,'changed.*inspection'):
            module.inspect_scripts(self.Inspector(replace=True))


if __name__ == "__main__":
    unittest.main()
