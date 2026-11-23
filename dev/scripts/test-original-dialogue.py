#!/usr/bin/env python3
"""Synthetic menus exercise actual public driver inputs and release commitment."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('dialogue', Path(__file__).with_name('original-dialogue.py'))
dialogue = importlib.util.module_from_spec(spec)
spec.loader.exec_module(dialogue)


class Session:
    def __init__(self):
        self.frame, self.world = 40, 1
        self.held, self.calls = set(), []
        self.menu = dict(active=True, highlighted=0, select_pending=False,
                         items=[dict(label='?one', action=7), dict(label='?two', action=12)],
                         committed=dict(label='?old', action=0))
        self.bad_ack, self.latch = False, True

    def snapshot(self):
        import copy
        return dict(frame=self.frame, world=self.world, dialogue_menu=copy.deepcopy(self.menu))

    def input(self, key, action):
        self.calls.append((self.frame, key, action))
        if action == 'press':
            self.held.add(key)
        else:
            self.held.discard(key)
        return self.frame + int(self.bad_ack)

    def advance_to(self, frame):
        self.frame = frame
        if 'ArrowDown' in self.held:
            self.menu['highlighted'] = (self.menu['highlighted'] + 1) % len(self.menu['items'])
        if 'ControlLeft' in self.held and self.latch:
            self.menu['select_pending'] = True
        elif self.menu['select_pending']:
            self.menu['committed'] = self.menu['items'][self.menu['highlighted']]
            self.menu['active'] = False
            self.menu['select_pending'] = False


class Topic(unittest.TestCase):
    def test_navigation_then_latched_release_commits_requested_installed_action(self):
        s = Session()
        result = dialogue.select_topic(s, '?two', 12)
        self.assertEqual(s.calls, [(40, 'ArrowDown', 'press'), (41, 'ArrowDown', 'release'),
                                  (41, 'ControlLeft', 'press'), (42, 'ControlLeft', 'release')])
        self.assertEqual(result['end']['frame'], 43)
        self.assertEqual(len(result['inputs']), 4)
        self.assertFalse(s.held)

    def test_key_acknowledgement_without_menu_latch_times_out_and_releases(self):
        s = Session()
        s.latch = False
        with self.assertRaisesRegex(RuntimeError, 'not committed.*pressed'):
            dialogue.select_topic(s, '?one', 7, 3)
        self.assertFalse(s.held)
        self.assertEqual(s.calls[-1], (43, 'ControlLeft', 'release'))

    def test_synchronous_next_menu_needs_observed_commit_before_table_replacement(self):
        def session():
            s = Session()
            advance = s.advance_to
            def replace(frame):
                advance(frame)
                if not s.menu['active']:
                    s.menu.update(active=True, items=[dict(label='?next', action=99)],
                                  committed=dict(label='?next', action=99))
            s.advance_to = replace
            return s
        s = session()
        with self.assertRaisesRegex(RuntimeError, 'menu changed before commitment'):
            dialogue.select_topic(s, '?one', 7)
        s = session()
        observed = []
        def commit():
            self.assertFalse(s.held)
            observed.append(s.frame)
            return dict(frame=s.frame, label='?one', action=7)
        result = dialogue.select_topic(s, '?one', 7, observe_commit=commit)
        self.assertEqual(observed, [41])
        self.assertEqual(result['commit_event'], dict(frame=41, label='?one', action=7))
        self.assertEqual(result['end']['dialogue_menu']['items'][0]['label'], '?next')
        self.assertFalse(s.held)

    def test_stale_or_wrong_event_cannot_acknowledge_release(self):
        for event in [dict(frame=40, label='?one', action=7),
                      dict(frame=41, label='?wrong', action=99)]:
            s = Session()
            with self.assertRaisesRegex(RuntimeError, 'wrong source frame|committed'):
                dialogue.select_topic(s, '?one', 7, observe_commit=lambda: event)
            self.assertFalse(s.held)

    def test_timeout_pauses_owned_boundary_before_releasing_held_input(self):
        s = Session()
        class Boundary:
            stopped = True
            def pause(self):
                self.stopped = True
        s.boundary = Boundary()
        original_input = s.input
        def input(key, action):
            self.assertTrue(s.boundary.stopped)
            return original_input(key, action)
        def timeout(frame):
            s.boundary.stopped = False
            raise TimeoutError('no ordinary boundary')
        s.input, s.advance_to = input, timeout
        with self.assertRaisesRegex(TimeoutError, 'no ordinary boundary'):
            dialogue.select_topic(s, '?one', 7)
        self.assertTrue(s.boundary.stopped)
        self.assertFalse(s.held)
        self.assertEqual(s.calls[-1], (40, 'ControlLeft', 'release'))

    def test_wrong_action_after_release_is_rejected(self):
        s = Session()
        advance = s.advance_to
        def wrong(frame):
            advance(frame)
            if not s.menu['active']:
                s.menu['committed'] = dict(label='?wrong', action=99)
        s.advance_to = wrong
        with self.assertRaisesRegex(RuntimeError, 'committed.*wrong'):
            dialogue.select_topic(s, '?one', 7)
        self.assertFalse(s.held)

    def test_changed_menu_or_world_rejects_and_releases(self):
        for mutate in [lambda s: s.menu['items'].append(dict(label='?new', action=1)),
                       lambda s: setattr(s, 'world', 2)]:
            s = Session()
            def advance(frame):
                s.frame = frame
                mutate(s)
            s.advance_to = advance
            with self.assertRaisesRegex(RuntimeError, 'changed'):
                dialogue.select_topic(s, '?two', 12)
            self.assertFalse(s.held)

    def test_missed_acknowledgement_releases_possibly_applied_key_before_any_update(self):
        s = Session()
        s.bad_ack = True
        with self.assertRaisesRegex(RuntimeError, 'expected frame 40, acknowledged 41'):
            dialogue.select_topic(s, '?one', 7)
        self.assertEqual(s.frame, 40)
        self.assertFalse(s.held)

    def test_absent_ambiguous_inactive_and_invalid_input_reject_before_input(self):
        for alter in [lambda s: s.menu.update(active=False),
                      lambda s: s.menu['items'].append(dict(label='?one', action=7)),
                      lambda s: setattr(s, 'world', -1)]:
            s = Session()
            alter(s)
            with self.assertRaises(ValueError):
                dialogue.select_topic(s, '?one', 7)
            self.assertFalse(s.calls)
        s = Session()
        for label, action, budget in [('?absent', 99, 3), ('?one', 7, 0), ('?one', '7', 3)]:
            with self.assertRaises(ValueError):
                dialogue.select_topic(s, label, action, budget)
        self.assertFalse(s.calls)


if __name__ == '__main__':
    unittest.main()
