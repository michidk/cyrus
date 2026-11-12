#!/usr/bin/env python3
"""Transition admission must surround observed gameplay, not a timed key pulse."""
import importlib.util
from pathlib import Path
import unittest
import tempfile

spec = importlib.util.spec_from_file_location('transition', Path(__file__).with_name('animation-transition.py'))
transition = importlib.util.module_from_spec(spec)
spec.loader.exec_module(transition)


def state(frame, group=0, owner='player', events=()):
    return dict(frame=frame, game_seconds=frame / 12, owner=owner, group=group, events=list(events))


class Admission(unittest.TestCase):
    def run_capture(self, rows):
        calls = []
        iterator = iter(rows)
        def observe():
            row = next(iterator)
            calls.append(('observe', row['frame']))
            return row
        def arm():
            calls.append(('arm',))
            return dict(armed=True, event_cursor=0, pcm_cursor=0)
        def key(held):
            calls.append(('key', held))
        def finish():
            calls.append(('finish',))
            return dict(frames=4096, sample_rate=22050)
        result = transition.capture(observe, arm, key, finish, 91, 22, 3, 1, lambda _: None)
        return result, calls

    def test_action_binding_comes_from_the_supplied_install(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'KEYS.INI'
            for scan in (31, 17):
                path.write_text(f'[input]\nquick_sword_key={scan}\n')
                self.assertEqual(transition.binding(path), scan)

    def test_unrelated_valueless_install_entry_does_not_hide_the_action_binding(self):
        with tempfile.TemporaryDirectory() as directory:
            path=Path(directory)/'KEYS.INI'
            path.write_text('[input]\nquick_sword_key=31\nunrelated_without_equal\n')
            self.assertEqual(transition.binding(path),31)

    def test_capture_is_armed_before_input_and_release_follows_group_entry(self):
        events = [dict(sequence=1, owner='player', kind='play', effect=22, slot=3),
                  dict(sequence=2, owner='player', kind='stop', slot=3)]
        result, calls = self.run_capture([state(1), state(2), state(3), state(4,91), state(5,91,events=events[:1]), state(6,events=events)])
        self.assertLess(calls.index(('arm',)), calls.index(('key',True)))
        self.assertLess(calls.index(('observe',4)), calls.index(('key',False)))
        self.assertEqual(result['entered']['frame'],4)
        self.assertEqual(result['completed']['frame'],6)
        self.assertEqual(result['events'],events)
        self.assertEqual(calls[-1],('finish',))

    def test_group_change_without_sound_evidence_is_rejected(self):
        with self.assertRaisesRegex(ValueError,'emission/stop'):
            self.run_capture([state(1),state(2),state(3),state(4,91),state(5)])

    def test_wrong_owner_or_reused_slot_cannot_borrow_event_success(self):
        for events in [
            [dict(sequence=1,owner='other',kind='play',effect=22,slot=3),dict(sequence=2,owner='other',kind='stop',slot=3)],
            [dict(sequence=1,owner='player',kind='play',effect=22,slot=3),dict(sequence=2,owner='player',kind='stop',slot=4)],
        ]:
            with self.assertRaisesRegex(ValueError,'emission/stop'):
                self.run_capture([state(1),state(2),state(3),state(4,91),state(5,events=events)])

    def test_skipped_group_rejects_at_game_progress_limit(self):
        with self.assertRaisesRegex(ValueError,'not observed'):
            self.run_capture([state(1),state(2),state(3),state(40)])

    def test_owner_replacement_releases_the_key_and_finalizes_failed_capture(self):
        calls=[]
        rows=iter([state(1),state(2),state(3),state(4,owner='replacement')])
        with self.assertRaisesRegex(ValueError,'replaced'):
            transition.capture(lambda:next(rows),lambda:dict(armed=True,event_cursor=0,pcm_cursor=0),
                lambda held:calls.append(('key',held)),lambda:calls.append(('finish',)) or {},
                91,22,3,1,lambda _:None)
        self.assertEqual(calls,[('key',True),('key',False),('finish',)])

    def test_arm_failure_never_presses_input(self):
        calls=[];rows=iter([state(1),state(2),state(3)])
        with self.assertRaisesRegex(ValueError,'armed'):
            transition.capture(lambda:next(rows),lambda:dict(armed=False),lambda held:calls.append(held),lambda:None,91,22,3,1,lambda _:None)
        self.assertEqual(calls,[])


if __name__ == '__main__':
    unittest.main()
