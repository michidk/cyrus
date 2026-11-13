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
    return dict(frame=frame, game_seconds=frame / 12, owner=owner, group=group, events=list(events),sound_overflow=False,animation='animation')


def service_events(owner='player',stop_slot=3):
    snapshot=dict(frame=5,game_seconds=5/12,owner='player',animation='animation',group=91,
                  mixer_generation=[0,0,0,1,1]+[0]*11)
    return [dict(sequence=1,kind='play',slot=3,generation=1,snapshot=snapshot),
            dict(sequence=2,kind='remember',slot=3,generation=1,snapshot=snapshot,owner=owner,effect=22),
            dict(sequence=3,kind='stop',slot=stop_slot,generation=1,snapshot=snapshot)]


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
        rows=list(rows)
        last=rows[-1]
        rows.append(state(last['frame']+1,last['group'],last['owner'],last['events']))
        iterator=iter(rows)
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
        events = service_events()
        result, calls = self.run_capture([state(1), state(2), state(3), state(4,91), state(5,91,events=events[:2]), state(6,events=events)])
        self.assertLess(calls.index(('arm',)), calls.index(('key',True)))
        self.assertLess(calls.index(('observe',4)), calls.index(('key',False)))
        self.assertEqual(result['entered']['frame'],4)
        self.assertEqual(result['completion_boundary']['frame'],6)
        self.assertEqual(result['completed']['frame'],7)
        self.assertEqual(result['events'],events)
        self.assertEqual(calls[-1],('finish',))

    def test_observed_mixer_completion_can_precede_handle_clear_without_a_stop(self):
        events=service_events();events[-1]['kind']='complete'
        result,_=self.run_capture([state(1),state(2),state(3),state(4,91),state(6,events=events)])
        self.assertEqual(result['sample_terminal'],'complete')

    def test_group_change_without_sound_evidence_is_rejected(self):
        with self.assertRaisesRegex(ValueError,'emission/stop'):
            self.run_capture([state(1),state(2),state(3),state(4,91),state(5)])

    def test_wrong_owner_or_reused_slot_cannot_borrow_event_success(self):
        for events in [service_events(owner='other'),service_events(stop_slot=4)]:
            with self.assertRaisesRegex(ValueError,'emission/stop'):
                self.run_capture([state(1),state(2),state(3),state(4,91),state(5,events=events)])

    def test_generation_reuse_or_event_loss_cannot_borrow_a_stop(self):
        for change in ('reuse','loss','overflow'):
            events=service_events()
            if change=='reuse':
                events[-1]['generation']=2
                events[-1]['snapshot']=dict(events[-1]['snapshot'],mixer_generation=[0,0,0,2]+[0]*12)
            if change=='loss':events.pop(0)
            row=state(6,events=events)
            if change=='overflow':row['sound_overflow']=True
            with self.subTest(change=change),self.assertRaises(ValueError):
                self.run_capture([state(1),state(2),state(3),state(4,91),row])

    def test_mixed_snapshot_cannot_supply_selected_owner_events(self):
        for change in [dict(owner='replaced'),dict(animation='replaced'),dict(frame=100),dict(game_seconds=100)]:
            events=service_events()
            events[0]['snapshot']=dict(events[0]['snapshot'],**change)
            with self.subTest(change=change),self.assertRaisesRegex(ValueError,'atomic selected-player'):
                self.run_capture([state(1),state(2),state(3),state(4,91),state(6,events=events)])

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

    def test_pcm_finalization_failure_preserves_observed_service_events(self):
        events=service_events();rows=iter([state(1),state(2),state(3),state(4,91),state(6,events=events),state(7,events=events)])
        saved=[]
        def fail():raise ValueError('PCM finalization failed')
        with self.assertRaisesRegex(ValueError,'PCM finalization failed'):
            transition.capture(lambda:next(rows),lambda:dict(armed=True,event_cursor=0,pcm_cursor=0),
                lambda held:None,fail,91,22,3,1,lambda value:saved.append(dict(value)))
        self.assertEqual(saved[-1]['events'],events)

    def test_arm_failure_never_presses_input(self):
        calls=[];rows=iter([state(1),state(2),state(3)])
        with self.assertRaisesRegex(ValueError,'armed'):
            transition.capture(lambda:next(rows),lambda:dict(armed=False),lambda held:calls.append(held),lambda:None,91,22,3,1,lambda _:None)
        self.assertEqual(calls,[])


if __name__ == '__main__':
    unittest.main()
