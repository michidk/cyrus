#!/usr/bin/env python3
"""The live adapter regression must select an original slot, including shared numbers."""
import importlib.util
import json
import unittest
import tempfile
from types import SimpleNamespace
from unittest.mock import patch
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


class LoadLifecycleTest(unittest.TestCase):
    class Control:
        def __init__(self, updates=2):
            self.calls = []
            self.updates = updates
            self.loaded = False
            self.book = False
            self.restored = False
            self.warmup = 0

        def ready(self, **kwargs):
            self.calls.append(("ready", kwargs))
            return self.entry()

        def entry(self):
            return {"state": "InGame", "scene_ready": True,
                    "scripts": {"ordinary_updates": self.updates, "restored": self.restored,
                                "world": 0, "content_id": "installed-start"},
                    "clock": {"continuation": {"warmup": self.warmup}}, "random_state": 21}

        def request(self, command):
            self.calls.append(command)
            if command.get("key") == "F3":
                self.book = True
            if command.get("key") == "Enter":
                self.loaded = True
            if command["type"] == "state":
                if self.loaded:
                    return {"state": "InGame", "scene_ready": True,
                            "scripts": {"restored": True, "world": 30,
                                        "ordinary_updates": 0, "failed": 0}}
                if not self.book:
                    return self.entry()
                return {"menu": {"ready": True, "page": 2, "selected": 0,
                                 "slots": [{"slot": 0, "source": {
                                     "type": "original", "world_id": 30}}],
                                 "entries": [{"action": 202}]}}
            return {}

    def test_scene_only_readiness_never_opens_book(self):
        control = self.Control(updates=0)
        with self.assertRaisesRegex(AssertionError, "established gameplay"):
            module.load_slot(control, SimpleNamespace(timeout=1))
        self.assertEqual(len(control.calls), 1)

    def test_load_waits_for_entry_then_captures_held_restoration(self):
        control = self.Control()
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(timeout=1, slot=0, world=30, output=Path(directory))
            with patch.object(module.driver, "capture_session") as capture:
                result = module.load_slot(control, args)
            self.assertEqual(control.calls[0], ("ready", {"timeout": 1, "advance": True}))
            self.assertIn(("ready", {"timeout": 1, "min_updates": 2, "advance": True}), control.calls)
            self.assertIn({"type": "input", "key": "F3", "action": "tap"}, control.calls)
            self.assertTrue(result["scripts"]["restored"])
            self.assertEqual(result["scripts"]["ordinary_updates"], 0)
            capture.assert_called_once_with(control, args.output / "admitted.session")
            self.assertTrue((args.output / "admission.json").is_file())


    def test_replay_held_entry_retries_transient_boundaries_before_opening_book(self):
        class HeldControl(self.Control):
            def __init__(self):
                super().__init__(updates=0)
                self.book = False
                self.attempts = 0

            ready = module.driver.Control.ready

            def response(self, command):
                self.calls.append(command)
                self.attempts += 1
                if self.attempts == 1:
                    return dict(ok=False, error='ordinary input boundary unavailable')
                self.updates += 1
                return dict(ok=True, result={})

            def request(self, command):
                if command.get('key') == 'F3':
                    self.book = True
                    assert self.updates >= 2
                if command['type'] == 'state' and not self.book:
                    result = self.entry()
                    result['examine'] = dict(open=False)
                    result['scripts']['failed'] = 0
                    return result
                return super().request(command)

        control = HeldControl()
        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(timeout=1, slot=0, world=30, output=Path(directory))
            with patch.object(module.driver, 'capture_session'), \
                    patch.object(module.driver.time, 'sleep'):
                result = module.load_slot(control, args)
        self.assertEqual(control.attempts, 3)
        self.assertTrue(result['scripts']['restored'])

    def test_one_update_is_not_the_replay_held_entry(self):
        control = self.Control(updates=1)
        with tempfile.TemporaryDirectory() as directory, patch.object(module.driver, "capture_session"):
            args = SimpleNamespace(timeout=1, slot=0, world=30, output=Path(directory))
            with self.assertRaisesRegex(AssertionError, "held gameplay"):
                module.load_slot(control, args)
        self.assertFalse(control.book)

    def test_observation_drift_rejects_before_opening_book(self):
        for field in ("updates", "random", "clock"):
            control = self.Control()
            request = control.request
            def drift(command):
                value = request(command)
                if command["type"] == "state" and not control.book:
                    if field == "updates": value["scripts"]["ordinary_updates"] += 1
                    if field == "random": value["random_state"] += 1
                    if field == "clock": value["clock"]["continuation"]["warmup"] += 1
                return value
            control.request = drift
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory, patch.object(module.driver, "capture_session"):
                args = SimpleNamespace(timeout=1, slot=0, world=30, output=Path(directory))
                with self.assertRaisesRegex(AssertionError, "entry.*changed"):
                    module.load_slot(control, args)
                self.assertFalse(control.book)

    def test_missing_continuation_rejects_before_opening_book(self):
        for field in ("clock", "random_state"):
            control = self.Control()
            ready = control.ready
            def incomplete(**kwargs):
                result = ready(**kwargs)
                del result[field]
                return result
            control.ready = incomplete
            with self.subTest(field=field), tempfile.TemporaryDirectory() as directory:
                args = SimpleNamespace(timeout=1, slot=0, world=30, output=Path(directory))
                with self.assertRaisesRegex(AssertionError, "continuation unavailable"):
                    module.load_slot(control, args)
                self.assertFalse(control.book)

    def test_restored_zero_update_entry_remains_eligible(self):
        control = self.Control(updates=0)
        control.restored = True
        with tempfile.TemporaryDirectory() as directory, patch.object(module.driver, "capture_session"):
            args = SimpleNamespace(timeout=1, slot=0, world=30, output=Path(directory))
            module.load_slot(control, args)
        self.assertTrue(control.book)

    def test_first_update_failure_retains_both_public_boundaries(self):
        admitted = {"scripts": {"ordinary_updates": 0}, "clock": {"continuation": {"frames": 3}}}
        failed = {"scripts": {"ordinary_updates": 1, "failed": 1}}
        actor_error = {"actors": [{"name": "opponent", "failure": "attack selector absent"}]}

        class Control:
            def request(self, command):
                if command["type"] == "state":
                    return failed if getattr(self, "advanced", False) else admitted
                self.advanced = True
                return {}

        with tempfile.TemporaryDirectory() as directory:
            args = SimpleNamespace(output=Path(directory), slot=0, world=30, frame_scale=200)
            with patch.object(module, "load_slot", return_value=admitted), \
                    patch.object(module, "inspect_scripts", side_effect=[{"actors": []}, actor_error]):
                with self.assertRaisesRegex(AssertionError, "attack selector absent"):
                    module.verify(Control(), args)
            report = json.loads((args.output / "result.json").read_text())
            self.assertEqual(report["admitted"], admitted)
            self.assertEqual(report["first_update"], failed)
            self.assertEqual(report["after_scripts"], actor_error)


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
