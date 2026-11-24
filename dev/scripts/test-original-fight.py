#!/usr/bin/env python3
"""Controls for the original-fight driver's pure logic, independent of the original and of game bytes."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('fight', Path(__file__).with_name('original-fight.py'))
fight = importlib.util.module_from_spec(spec)
spec.loader.exec_module(fight)


def actor(identifier, group, health, selected=False, pending=None, drawn=0):
    return {"id": identifier, "selected": selected, "group": group, "health": health,
            "pending": pending, "drawn": drawn, "position_q8": [0, 0, 0], "yaw": 0}


def sample(pg=8, rg=8, rh=250, drawn=1, ph=100):
    return {"frame": 100, "fps": 12, "pg": pg, "ph": ph, "pdrawn": drawn, "rg": rg, "rpending": None, "rh": rh}


class DriverLogic(unittest.TestCase):
    def test_query_position_selects_attachment_before_valid_link_and_local(self):
        raw = actor(1, 8, 100) | {"query_spatial": {
            "slave": False, "attached_q8": [1, 2, 3],
            "link_valid": True, "linked_q8": [4, 5, 6]}}
        point = fight.QueryPosition.read(raw)
        self.assertEqual((point.source.value, point.position_q8), ("linked", (4, 5, 6)))
        raw['query_spatial']['slave'] = True
        self.assertEqual(fight.QueryPosition.read(raw).position_q8, (1, 2, 3))
        self.assertEqual(fight.QueryPosition.read(raw).source.value, 'attached')
        raw['query_spatial'].update(slave=False, link_valid=False)
        self.assertEqual(fight.QueryPosition.read(raw).position_q8, (0, 0, 0))
        self.assertEqual(fight.QueryPosition.read(raw).source.value, 'local')

    def test_reader_to_view_preserves_query_owners_instead_of_body_origins(self):
        player = actor(1, 8, 100, selected=True) | {'query_spatial': {
            'slave': True, 'attached_q8': [0, 0, 0]}}
        opponent = actor(55, 8, 250) | {
            'query_spatial': {'slave': False, 'link_valid': True, 'linked_q8': [3, 4, 0]},
            'combat_state': {'contact_radius_q8': 0}}
        state = fight.view({'actors': [player, opponent], 'counters': {'mainloop_frame': 9, 'fps': 12}})
        result = fight.combat_query_range_sample(state, {
            'current_col_sphere_size': 0, 'min_range': 0, 'max_range': 6})
        self.assertEqual((result['frame'], result['distance_q8'], result['strict_range_pass']), (9, 5, True))
        self.assertEqual(result['player_query']['source'], 'attached')
        self.assertEqual(result['opponent_query']['source'], 'linked')

    def test_query_position_rejects_unbound_selected_source(self):
        for spatial in [None, {}, {"slave": False},
                        {"slave": True, "link_valid": False},
                        {"slave": False, "link_valid": True}]:
            with self.subTest(spatial=spatial), self.assertRaises(ValueError):
                fight.QueryPosition.read(actor(1, 8, 100) | {"query_spatial": spatial})

    def test_contact_range_uses_query_positions_height_and_strict_endpoints(self):
        state = sample() | {"player_query": {"source": "linked", "position_q8": [0, 0, 0]},
                            "opponent_query": {"source": "attached", "position_q8": [3, 4, 0]},
                            "ppos": [999, 999, 999], "rpos": [999, 999, 999],
                            "opponent_combat": {"contact_radius_q8": 0, "flags": 2}}
        motion = {"current_col_sphere_size": 0, "min_range": 0, "max_range": 5}
        result = fight.combat_query_range_sample(state, motion)
        self.assertEqual(result['distance_q8'], 5)
        self.assertFalse(result['strict_range_pass'])
        motion['max_range'] = 6
        self.assertTrue(fight.combat_query_range_sample(state, motion)['strict_range_pass'])
        motion['min_range'] = 5
        self.assertFalse(fight.combat_query_range_sample(state, motion)['strict_range_pass'])
        # This is only a range observation. Active defence cannot turn it into a miss verdict.
        self.assertNotIn('contact', result)
        self.assertNotIn('hit', result)

    def test_contact_range_wraps_and_rounds_stored_single_precision_root(self):
        state = {"player_query": {"source": "local", "position_q8": [2147483647, 0, 0]},
                 "opponent_query": {"source": "local", "position_q8": [-2147483648, 0, 0]},
                 "opponent_combat": {"contact_radius_q8": 0}}
        motion = {"current_col_sphere_size": 0, "min_range": 0, "max_range": 2}
        self.assertEqual(fight.combat_query_range_sample(state, motion)['distance_q8'], 1)
        state['player_query']['position_q8'] = [0, 0, 0]
        state['opponent_query']['position_q8'] = [16777217, 0, 0]
        self.assertEqual(fight.combat_query_range_sample(state, motion)['distance_q8'], 16777216)

    def test_contact_range_rejects_other_contact_branches_and_missing_bindings(self):
        state = {"player_query": {"source": "local", "position_q8": [0, 0, 0]},
                 "opponent_query": {"source": "local", "position_q8": [1, 0, 0]},
                 "opponent_combat": {"contact_radius_q8": 0}}
        motion = {"current_col_sphere_size": 0, "min_range": 0, "max_range": 2}
        for key in ('player_query', 'opponent_query', 'opponent_combat'):
            with self.subTest(key=key), self.assertRaises(ValueError):
                fight.combat_query_range_sample({k: v for k, v in state.items() if k != key}, motion)
        with self.assertRaises(ValueError):
            fight.combat_query_range_sample(state, motion | {"current_col_sphere_size": 1})
        with self.assertRaises(ValueError):
            fight.combat_query_range_sample(state | {"opponent_combat": {"contact_radius_q8": 1}}, motion)

    def test_attack_events_separate_cancellation_contact_and_health_changes(self):
        def state(frame, flags, cursor, ph=100, rh=250, defence=0):
            return sample(pg=102, ph=ph, rh=rh) | {
                "frame": frame, "player_command_cursor": cursor,
                "player_combat": {"flags": flags},
                "opponent_combat": {"flags": defence}}
        rows = [state(10, 5, 1), state(11, 4, 2, ph=90),
                state(12, 4, 7, ph=90)]
        events = fight.combat_events(rows)
        self.assertEqual(events, [
            {"frame": 11, "event": "player_intent_cleared", "cursor": 2, "group": 102},
            {"frame": 11, "event": "player_health_changed", "before": 100, "after": 90}])
        rows = [state(20, 5, 6), state(21, 36, 7, defence=2),
                state(22, 4, 8, rh=240)]
        events = fight.combat_events(rows)
        self.assertIn({"frame": 21, "event": "player_intent_cleared", "cursor": 7, "group": 102}, events)
        self.assertIn({"frame": 21, "event": "player_contact_tested", "cursor": 7, "group": 102}, events)
        self.assertIn({"frame": 21, "event": "opponent_defence_started"}, events)
        self.assertIn({"frame": 22, "event": "opponent_health_changed", "before": 250, "after": 240}, events)

    def test_attack_events_reject_gaps_and_do_not_invent_unbound_intent(self):
        self.assertEqual(fight.combat_events([sample() | {"frame": 1}, sample() | {"frame": 2}]), [])
        with self.assertRaisesRegex(ValueError, "consecutive"):
            fight.combat_events([sample() | {"frame": 1}, sample() | {"frame": 3}])

    def test_view_reads_both_actors_and_tolerates_a_late_opponent(self):
        state = {"counters": {"mainloop_frame": 7, "fps": 4}, "actors": [
            actor(1, 8, 100, selected=True, drawn=1), actor(55, 102, 240, pending=8)]}
        view = fight.view(state)
        self.assertEqual((view["frame"], view["pg"], view["rg"], view["rh"], view["rpending"]), (7, 8, 102, 240, 8))
        state["actors"].pop()
        view = fight.view(state)
        self.assertEqual((view["pg"], view["rg"], view["rh"]), (8, None, None))
        self.assertIsNone(fight.view({"counters": {"mainloop_frame": 1, "fps": 1}, "actors": [actor(55, 8, 250)]}))
        self.assertIsNone(fight.view(None))
        self.assertIsNone(fight.view({"error": "world changing"}))

    def test_camera_list_absence_preserves_independent_actor_observation(self):
        opponent = actor(55, 93, 250) | {"hidden": False, "death_selector": 6,
                                               "in_update_list": False}
        raw = {"counters": {"mainloop_frame": 20, "fps": 4},
               "actors": [actor(1, 8, 100, selected=True)],
               "observed_actors": [opponent]}
        view = fight.view(raw)
        self.assertEqual((view["rh"], view["rg"], view["rcombat"]), (250, 93, True))
        self.assertFalse(view["rlisted"])
        raw["actors"].append(opponent)
        self.assertTrue(fight.view(raw)["rlisted"])

    def test_view_retains_contact_and_defence_observations_without_inferring_from_groups(self):
        player = actor(1, 102, 100, selected=True) | {
            "command_frame": 7, "combat_state": {"flags": 33, "reverse": 0}}
        opponent = actor(55, 99, 250) | {
            "command_frame": 3, "combat_state": {"flags": 6, "defence": 2}}
        raw = {"actors": [player, opponent], "counters": {"mainloop_frame": 1, "fps": 12}}
        view = fight.view(raw)
        self.assertEqual(view["player_combat"], player["combat_state"])
        self.assertEqual(view["opponent_combat"], opponent["combat_state"])
        self.assertEqual(view["player_command_cursor"], 7)
        self.assertEqual(view["opponent_command_cursor"], 3)
        del opponent["combat_state"]
        self.assertIsNone(fight.view(raw)["opponent_combat"])

    def test_a_tap_needs_the_armed_idle_animation_and_a_drawn_sword(self):
        for gate in ("busy", "any"):
            self.assertFalse(fight.ready_to_tap(sample(pg=102, rg=102), gate))
            self.assertFalse(fight.ready_to_tap(sample(pg=0, rg=102), gate))
            # The reader's drawn byte stays 0 in the armed idle group, so it must not gate a tap.
        self.assertTrue(fight.ready_to_tap(sample(drawn=0, rg=102), "busy"))
        self.assertTrue(fight.ready_to_tap(sample(drawn=0, rg=8), "any"))
        self.assertTrue(fight.ready_to_tap(sample(rg=102), "busy"))
        self.assertTrue(fight.ready_to_tap(sample(rg=8), "any"))

    def test_the_busy_gate_waits_while_the_opponent_is_idle_unengaged_or_absent(self):
        for group in (8, 0, None):
            self.assertFalse(fight.ready_to_tap(sample(rg=group), "busy"))
        for group in (91, 92, 98, 102, 117):
            self.assertTrue(fight.ready_to_tap(sample(rg=group), "busy"))

    def test_tap_outcomes_separate_hits_reactions_responses_and_misses(self):
        before = sample(rg=102, rh=250)
        self.assertEqual(fight.classify_tap(before, [sample(rh=250), sample(rh=240)]), {"outcome": "hit", "damage": 10})
        self.assertEqual(fight.classify_tap(before, [sample(rh=250), sample(rh=220)])["damage"], 30)
        self.assertEqual(fight.classify_tap(before, [sample(rg=117, rh=250)])["outcome"], "reaction")
        response = fight.classify_tap(before, [sample(rg=98, rh=250), sample(rg=99, rh=250)])
        self.assertEqual((response["outcome"], response["groups"]), ("response", [98, 99]))
        self.assertEqual(fight.classify_tap(before, [sample(rg=102, rh=250)])["outcome"], "miss")
        self.assertEqual(fight.classify_tap(before, [])["outcome"], "miss")

    def test_summary_counts_taps_by_opponent_state_and_lists_damage_per_hit(self):
        def tap(rg, outcome, damage=0):
            return {"ev": "tap", "before": sample(rg=rg), "outcome": outcome, "damage": damage}
        rows = [tap(102, "hit", 10), tap(102, "miss"), tap(8, "response"), tap(8, "hit", 20),
                {"ev": "end", "rh": 220, "ph": 100, "frames": 900, "reason": "frame-budget"}]
        summary = fight.summarize(rows)
        self.assertEqual((summary["taps"], summary["hits"], summary["damage"]), (4, 2, [10, 20]))
        self.assertEqual(summary["by_state"]["busy"], {"taps": 2, "hit": 1, "reaction": 0, "response": 0, "miss": 1})
        self.assertEqual(summary["by_state"]["idle"], {"taps": 2, "hit": 1, "reaction": 0, "response": 1, "miss": 0})
        self.assertEqual(summary["end"]["reason"], "frame-budget")


class StateRows(unittest.TestCase):
    def test_view_carries_q8_positions_and_the_horizontal_distance_ignores_height(self):
        state = {"counters": {"mainloop_frame": 7, "fps": 4}, "actors": [
            dict(actor(1, 8, 100, selected=True), position_q8=[0, -5000, 0], yaw=1024),
            dict(actor(55, 102, 240), position_q8=[3000, 90000, 4000], yaw=0)]}
        view = fight.view(state)
        self.assertEqual((view["ppos"], view["rpos"], view["pyaw"], view["ryaw"]),
                         ([0, -5000, 0], [3000, 90000, 4000], 1024, 0))
        self.assertEqual(fight.horizontal_distance(view), 5000)
        state["actors"].pop()
        absent = fight.view(state)
        self.assertIsNone(absent["rpos"])
        self.assertIsNone(fight.horizontal_distance(absent))


class WalkIn(unittest.TestCase):
    def sample_at(self, ppos, pyaw, rpos):
        return sample() | {"ppos": ppos, "pyaw": pyaw, "rpos": rpos}

    def test_the_walk_in_turns_toward_him_then_walks_then_stops_in_range(self):
        # He is straight ahead on +Z (bearing 0, wanted yaw 0).
        ahead = self.sample_at([0, 0, 0], 0, [0, 0, 50000])
        self.assertEqual(fight.walk_step(ahead), ("Up", 1.0))
        # He is on +X (bearing 512, wanted yaw 1536): a player at yaw 0 must turn; Right decreases yaw.
        side = self.sample_at([0, 0, 0], 0, [50000, 0, 0])
        key, hold = fight.walk_step(side)
        self.assertEqual(key, "Right")
        self.assertTrue(0.2 <= hold <= 1.5)
        left = self.sample_at([0, 0, 0], 1200, [50000, 0, 0])
        self.assertEqual(fight.walk_step(left)[0], "Left")
        # Inside range and faced: stop. Opponent absent: nothing to do.
        self.assertIsNone(fight.walk_step(self.sample_at([0, 0, 0], 0, [0, 0, 9000])))
        self.assertIsNone(fight.walk_step(self.sample_at([0, 0, 0], 0, None)))


class CombatEntryObservation(unittest.TestCase):
    def test_allocated_combat_structure_does_not_mean_begin_combat_has_run(self):
        enemy = dict(actor(55, 0, 250), hidden=False, combat_owner=123, death_selector=0)
        raw = {"world": 30, "counters": {"mainloop_frame": 1, "fps": 12},
               "actors": [actor(1, 0, 100, selected=True), enemy]}
        self.assertFalse(fight.view(raw)["rcombat"])
        enemy["death_selector"] = 6
        self.assertTrue(fight.view(raw)["rcombat"])
        self.assertFalse(fight.view(raw)["rhidden"])


class EngageGate(unittest.TestCase):
    def test_player_distance_sums_all_three_axes_like_the_script_gate(self):
        near = sample() | {"ppos": [0, 0, 0], "rpos": [30000, 40000, 20000]}
        self.assertEqual(fight.player_distance_q8(near), 90000)
        self.assertLess(fight.player_distance_q8(near), fight.ENGAGE_RANGE_Q8)
        below = sample() | {"ppos": [0, -354048, 0], "rpos": [12000, -826880, 8000]}
        self.assertEqual(fight.player_distance_q8(below), 12000 + 472832 + 8000)
        self.assertGreater(fight.player_distance_q8(below), fight.ENGAGE_RANGE_Q8)
        self.assertIsNone(fight.player_distance_q8(sample() | {"rpos": None}))
        self.assertEqual(fight.ENGAGE_RANGE_Q8, 512 * 256)

    def test_the_walk_in_stops_with_the_reason_when_height_alone_exceeds_the_gate(self):
        class Sampler:
            def get(self):
                return sample(rg=0) | {"frame": 10, "ppos": [0, -354048, 0], "rpos": [12000, -826880, 8000]}, {}

        with self.assertRaisesRegex(AssertionError, "1847 map units above or below"):
            fight.walk_in(Sampler(), lambda *a: None, 3000)


class ExactCombatTape(unittest.TestCase):
    class Session:
        def __init__(self, gap=False):
            self.frame, self.keys, self.gap = 0, set(), gap
        def snapshot(self):
            return sample() | {"frame": self.frame, "ppos": [0, 0, 0], "rpos": [0, 0, 100],
                               "world": 30, "rhidden": False, "rcombat": True, "rlisted": True, "lockout": 0}
        def advance_to(self, frame):
            self.frame = frame + int(self.gap)
        def input(self, key, action):
            if action == "press": self.keys.add(key)
            else: self.keys.discard(key)

    def test_tap_records_every_update_and_releases(self):
        session = self.Session()
        result = fight.record_combat_tap(session, "ControlLeft", 1, 3)
        self.assertEqual([s["frame"] for s in result["samples"]], [0, 1, 2, 3, 4])
        self.assertEqual(session.keys, set())
        self.assertEqual(result["outcome"], "miss")

    def test_frame_gap_rejects_measurement_and_releases_key(self):
        session = self.Session(gap=True)
        with self.assertRaisesRegex(ValueError, "frame gap"):
            fight.record_combat_tap(session, "ControlLeft", 1, 3)
        self.assertEqual(session.keys, set())

    def test_busy_player_does_not_count_as_an_admitted_attack(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | {"pg": 102}
        with self.assertRaisesRegex(ValueError, "armed idle"):
            fight.record_combat_tap(session, "ControlLeft", 1, 3)
        self.assertEqual(session.keys, set())

    def test_scripted_player_lockout_rejects_attack_measurement(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | {"lockout": 1}
        with self.assertRaisesRegex(ValueError, "player control"):
            fight.record_combat_tap(session, "ControlLeft", 1, 3)
        self.assertEqual(session.keys, set())

    def test_leaving_update_list_during_tap_rejects_and_releases(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | {"rlisted": session.frame == 0}
        with self.assertRaisesRegex(ValueError, "left the ordinary update list"):
            fight.record_combat_tap(session, "ControlLeft", 1, 3)
        self.assertEqual(session.keys, set())

    def test_off_list_observation_is_not_an_admitted_fight(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | {"rlisted": False}
        with self.assertRaisesRegex(ValueError, "update list"):
            fight.record_combat_tap(session, "ControlLeft", 1, 3)
        self.assertEqual(session.keys, set())

    def test_hidden_opponent_is_not_an_admitted_fight(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | {"rhidden": True}
        with self.assertRaisesRegex(ValueError, "revealed.*combat"):
            fight.record_combat_tap(session, "ControlLeft", 1, 3)
        self.assertEqual(session.keys, set())


class CombatRangeTape(unittest.TestCase):
    class Session(ExactCombatTape.Session):
        def snapshot(self):
            return super().snapshot() | {"pyaw": 0,
                "ppos": [0, 0, -10 * self.frame],
                "rpos": [0, 12, 100 - 10 * self.frame]}

    def test_constant_range_does_not_hide_both_actors_moving(self):
        session = self.Session()
        result = fight.record_combat_tap(session, 'ArrowDown', 1, 2, capture_range=True)
        self.assertEqual((result['key'], result['release_frame']), ('ArrowDown', 1))
        rows = result['trajectory']
        self.assertEqual([row['frame'] for row in rows], [0, 1, 2, 3])
        self.assertEqual([row['l1_q8'] for row in rows], [112] * 4)
        self.assertEqual(rows[1]['player_delta_q8'], [0, 0, -10])
        self.assertEqual(rows[1]['opponent_delta_q8'], [0, 0, -10])
        self.assertIsNone(rows[0]['player_delta_q8'])
        self.assertEqual(rows[1]['height_delta_q8'], 12)
        self.assertNotIn('hit', rows[1])
        self.assertEqual(session.keys, set())

    def test_missing_pose_rejects_before_input_instead_of_inventing_zero_motion(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | {'rpos': None}
        with self.assertRaisesRegex(ValueError, 'bound.*positions'):
            fight.record_combat_tap(session, 'ArrowDown', 1, 1, capture_range=True)
        self.assertEqual(session.frame, 0)
        self.assertEqual(session.keys, set())

    def test_failed_range_read_releases_and_rejects_incomplete_measurement(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | ({'pyaw': None} if session.frame else {})
        with self.assertRaisesRegex(ValueError, 'heading'):
            fight.record_combat_tap(session, 'ArrowDown', 1, 1, capture_range=True)
        self.assertEqual(session.keys, set())

    def test_reveal_loss_during_range_tap_rejects_and_releases(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | {'rhidden': session.frame > 0}
        with self.assertRaisesRegex(ValueError, 'reveal/combat admission'):
            fight.record_combat_tap(session, 'ArrowDown', 1, 1, capture_range=True)
        self.assertEqual(session.keys, set())

    def test_control_loss_during_range_tap_rejects_and_releases(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | {'lockout': int(session.frame > 0)}
        with self.assertRaisesRegex(ValueError, 'player control'):
            fight.record_combat_tap(session, 'ArrowDown', 1, 1, capture_range=True)
        self.assertEqual(session.keys, set())


class CombatQueryTape(unittest.TestCase):
    class Session(ExactCombatTape.Session):
        def snapshot(self):
            return super().snapshot() | {
                'player_query': {'source': 'local', 'position_q8': [0, 0, 0]},
                'opponent_query': {'source': 'linked', 'position_q8': [0, 0, 100 + self.frame]},
                'opponent_combat': {'contact_radius_q8': 0, 'flags': 2}}

    def test_tape_retains_query_range_separately_from_body_geometry(self):
        session = self.Session()
        result = fight.record_combat_tap(session, 'ControlLeft', 1, 2,
            query_motion={'current_col_sphere_size': 0, 'min_range': 100, 'max_range': 103})
        self.assertEqual([v['distance_q8'] for v in result['query_ranges']], [100, 101, 102, 103])
        self.assertEqual([v['strict_range_pass'] for v in result['query_ranges']], [False, True, True, False])
        self.assertIsNone(result['trajectory'])
        self.assertEqual(session.keys, set())

    def test_lost_query_binding_during_tap_rejects_and_releases(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | ({'opponent_query': None} if session.frame else {})
        with self.assertRaisesRegex(ValueError, 'opponent query position unavailable'):
            fight.record_combat_tap(session, 'ControlLeft', 2, 1,
                query_motion={'current_col_sphere_size': 0, 'min_range': 0, 'max_range': 200})
        self.assertEqual(session.keys, set())


class DefenceAdmissionEvidence(unittest.TestCase):
    def state(self):
        return {'player_combat': {'action': 0}, 'opponent_combat': {'defence': 1, 'flags': 2},
                'player_command_cursor': 7}

    def motion(self):
        return {'defense': 2, 'first_defend_frame': 2, 'defend_duration': 7}

    def test_state_admission_stops_before_unobserved_contact(self):
        gates = fight.defence_admission_sample(self.state(), self.motion())
        self.assertEqual(gates['counter_intent'], 'passed_by_state')
        self.assertEqual(gates['contact'], 'unobserved')

    def test_selector_mismatch_precedes_window_and_never_implies_geometry_miss(self):
        state = self.state() | {'opponent_combat': {'defence': -1, 'flags': 5}}
        gates = fight.defence_admission_sample(state, self.motion())
        self.assertEqual(gates['defence_selector'], 'rejected_by_state')
        self.assertEqual(gates['defence_window'], 'not_reached')
        self.assertEqual(gates['contact'], 'not_reached')

    def test_window_end_is_exclusive_and_missing_data_is_not_a_failure_verdict(self):
        for cursor, expected in [(1, 'rejected_by_state'), (2, 'passed_by_state'),
                                 (8, 'passed_by_state'), (9, 'rejected_by_state'), (None, 'unobserved')]:
            with self.subTest(cursor=cursor):
                gates = fight.defence_admission_sample(self.state() | {'player_command_cursor': cursor}, self.motion())
                self.assertEqual(gates['defence_window'], expected)
        gates = fight.defence_admission_sample(self.state(), self.motion() | {'defend_duration': None})
        self.assertEqual(gates['defence_window'], 'unobserved')

    def test_inactive_actions_and_missing_counter_intent_reject_before_contact(self):
        for action in (-1, 9):
            gates = fight.defence_admission_sample(self.state() | {'player_combat': {'action': action}}, self.motion())
            self.assertEqual(gates['active_attacker'], 'rejected_by_state')
        gates = fight.defence_admission_sample(self.state() | {'opponent_combat': {'defence': 1, 'flags': 0}}, self.motion())
        self.assertEqual(gates['counter_intent'], 'rejected_by_state')
        self.assertEqual(gates['contact'], 'not_reached')


class DefenceResponseEvidence(unittest.TestCase):
    def events(self):
        state = {'world': 30, 'player_combat': {'action': 0, 'flags': 5, 'reverse': 1},
                 'opponent_combat': {'flags': 7, 'contact_radius_q8': 0}}
        branch = {'event': 'defence_contact_pass_reverse_write', 'frame': 70,
                  'attacker': 'player', 'defender': 'opponent',
                  'contact_path': 'zero_radius_range', 'state': state}
        result = {'event': 'defence_success_result_write', 'frame': 70,
                  'attacker': 'player', 'defender': 'opponent', 'result': 1,
                  'state': state | {'player_combat': {'flags': 4, 'reverse': 1},
                                    'opponent_combat': {'flags': 12}}}
        return branch, result

    def test_bound_branch_and_success_write_prove_gates_and_ordered_response(self):
        report = fight.defence_response_evidence(*self.events())
        self.assertEqual(report['contact_path'], 'zero_radius_range')
        self.assertEqual(report['gates']['elevation'], 'passed_by_executed_branch')
        self.assertEqual(report['gates']['orientation'], 'passed_by_executed_branch')
        self.assertEqual(report['result'], 1)
        self.assertEqual(report['response'], 'completed')
        self.assertNotIn('damage', report)

    def test_unrelated_flag_bits_survive_the_response(self):
        branch, result = self.events()
        branch['state']['player_combat']['flags'] = 0xb5
        branch['state']['opponent_combat']['flags'] = 0xa7
        result['state']['player_combat']['flags'] = 0xb4
        result['state']['opponent_combat']['flags'] = 0xac
        self.assertEqual(fight.defence_response_evidence(branch, result)['response'], 'completed')

    def test_post_update_reversal_alone_is_not_branch_evidence(self):
        branch, result = self.events()
        for invalid in [branch | {'event': 'snapshot'}, branch | {'contact_path': None},
                        branch | {'attacker': 'opponent'}, branch | {'state': {}}]:
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                fight.defence_response_evidence(invalid, result)

    def test_success_requires_same_update_world_pair_and_actual_flag_consumption(self):
        branch, result = self.events()
        invalids = [result | {'frame': 71}, result | {'result': 0}, result | {'defender': 'player'},
                    result | {'state': result['state'] | {'world': 31}},
                    result | {'state': result['state'] | {'opponent_combat': {'flags': 4}}},
                    result | {'state': result['state'] | {'player_combat': {'flags': 5, 'reverse': 1}}}]
        for invalid in invalids:
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                fight.defence_response_evidence(branch, invalid)


class CombatAlignment(unittest.TestCase):
    class Session(ExactCombatTape.Session):
        def snapshot(self):
            return super().snapshot() | {"ppos": [0, 0, 0], "rpos": [0, 0, -100],
                                        "pyaw": [800, 1000, 1030][min(self.frame, 2)]}

    def test_a_broad_forward_arc_does_not_end_alignment_before_center_crossing(self):
        session = self.Session()
        aligned = fight.align_combat_facing(session, 3)
        self.assertEqual(aligned['frame'], 2)
        self.assertEqual(fight.combat_geometry(aligned)['heading_error'], -6)
        self.assertEqual(session.keys, set())

    def test_alignment_failure_releases_turn_key(self):
        session = self.Session()
        with self.assertRaisesRegex(TimeoutError, 'center'):
            fight.align_combat_facing(session, 1)
        self.assertEqual(session.keys, set())

    def test_alignment_rejects_world_change_and_releases(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | {"world": 30 if session.frame == 0 else 1}
        with self.assertRaisesRegex(ValueError, 'changed world'):
            fight.align_combat_facing(session, 3)
        self.assertEqual(session.keys, set())

    def test_recovery_uses_action_and_defence_state_not_an_animation_group(self):
        session = self.Session()
        snapshot = session.snapshot
        session.snapshot = lambda: snapshot() | {"rg": 108,
            "opponent_combat": {"action": 1 if session.frame < 2 else -1,
                                "flags": 5 if session.frame < 2 else 4}}
        ready = fight.wait_for_combat_recovery(session, 3)
        self.assertEqual(ready['frame'], 2)
        self.assertEqual(session.keys, set())

    def test_unbound_combat_state_cannot_be_treated_as_recovery(self):
        session = self.Session()
        with self.assertRaisesRegex(ValueError, 'combat observations'):
            fight.wait_for_combat_recovery(session, 3)

    def test_geometry_retains_height_and_is_not_a_contact_verdict(self):
        state = dict(ppos=[0, 0, 0], rpos=[3000, 12000, 4000], pyaw=0)
        geometry = fight.combat_geometry(state)
        self.assertEqual(geometry['horizontal_q8'], 5000)
        self.assertEqual(geometry['distance_q8'], 13000)
        self.assertEqual(geometry['height_delta_q8'], 12000)
        self.assertNotIn('contact', geometry)


class FakeSampler:
    """Yields one scripted (view, raw) pair per get(): frame counter and opponent group."""

    def __init__(self, steps):
        self.steps = list(steps)

    def get(self):
        frame, group = self.steps.pop(0) if len(self.steps) > 1 else self.steps[0]
        raw = {"counters": {"mainloop_frame": frame}}
        return (sample(rg=group) | {"frame": frame}), raw


class FrameWaits(unittest.TestCase):
    def test_a_condition_met_inside_the_frame_budget_returns_that_sample(self):
        sampler = FakeSampler([(10, None), (20, None), (30, 102)])
        result = fight.wait_frames(sampler, 100, 5, until=lambda s: s["rg"] == 102)
        self.assertEqual(result["frame"], 30)

    def test_the_frame_budget_not_wall_time_ends_a_wait_with_a_condition(self):
        sampler = FakeSampler([(10, None), (60, None), (130, None)])
        with self.assertRaisesRegex(TimeoutError, "within 100 frames"):
            fight.wait_frames(sampler, 100, 5, until=lambda s: s["rg"] == 102)

    def test_a_plain_frame_wait_returns_once_the_counter_has_advanced(self):
        sampler = FakeSampler([(10, 8), (14, 8), (21, 8)])
        self.assertEqual(fight.wait_frames(sampler, 10, 5)["frame"], 21)


class OriginalFrameSession(unittest.TestCase):
    def test_quicksave_configuration_is_private_and_explicit(self):
        import tempfile
        scenario_spec = importlib.util.spec_from_file_location(
            'scenario', Path(__file__).with_name('original-scenario.py'))
        scenario = importlib.util.module_from_spec(scenario_spec)
        scenario_spec.loader.exec_module(scenario)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            run = root / 'run'
            (run / 'Redguard').mkdir(parents=True)
            config = run / 'Redguard/SYSTEM.INI'
            config.write_text('[debug]\nfinal_version=0\n')
            scenario.enable_quicksave(run)
            self.assertEqual(config.read_text(), '[debug]\nfinal_version=1\n')
            external = root / 'shared.ini'
            external.write_text('final_version=0\n')
            config.unlink()
            config.symlink_to(external)
            with self.assertRaisesRegex(ValueError, 'run-local'):
                scenario.enable_quicksave(run)
            self.assertEqual(external.read_text(), 'final_version=0\n')

    def test_quicksave_waits_for_two_game_frames_and_copies_private_slot(self):
        import subprocess
        import tempfile
        from unittest.mock import patch
        scenario_spec = importlib.util.spec_from_file_location(
            'scenario', Path(__file__).with_name('original-scenario.py'))
        scenario = importlib.util.module_from_spec(scenario_spec)
        scenario_spec.loader.exec_module(scenario)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            game = scenario.Original.__new__(scenario.Original)
            game.run = root / 'run'
            slot = game.run / 'Redguard/SAVEGAME/SAVEGAME.000'
            slot.mkdir(parents=True)
            source = slot / 'SAVEGAME.SAV'
            source.write_bytes(b'old synthetic save')
            keys = []
            def key(*args):
                keys.append(args)
                source.write_bytes(b'new synthetic save')
                return subprocess.CompletedProcess(args, 0)
            frames = iter([10, 10, 11, 12])
            game.key = key
            game.frame = lambda: next(frames)
            with patch.object(scenario.time, 'sleep'):
                receipt = game.capture_quicksave(root / 'copy')
            self.assertEqual(receipt['frame'], 12)
            self.assertEqual((root / 'copy/SAVEGAME.SAV').read_bytes(), b'new synthetic save')
            self.assertEqual(keys, [('key', '--delay', '200', 'F9')])
            source.unlink()
            shared = root / 'shared'
            shared.write_bytes(b'untouched')
            source.symlink_to(shared)
            with self.assertRaisesRegex(ValueError, 'linked save content'):
                with patch.object(scenario.time, 'sleep'):
                    game.frame = iter([20, 22]).__next__
                    game.key = lambda *args: subprocess.CompletedProcess(args, 0)
                    # A linked save must be rejected before any key or read.
                    game.capture_quicksave(root / 'bad-copy')
            self.assertEqual(shared.read_bytes(), b'untouched')

    def test_real_key_names_and_frame_waits_use_the_stream_counter(self):
        import subprocess
        scenario_spec = importlib.util.spec_from_file_location(
            'scenario', Path(__file__).with_name('original-scenario.py'))
        scenario = importlib.util.module_from_spec(scenario_spec)
        scenario_spec.loader.exec_module(scenario)
        class Game:
            count = 80
            keys = []
            def frame(self):
                return self.count
            def raw(self):
                return {'counters': {'mainloop_frame': self.count}}
            def wait(self, frames, until=None):
                # A frame may finish after advance_to samples the counter.
                self.count += 1
                if until:
                    while not until({"frame": self.count}):
                        self.count += 1
                else:
                    self.count += frames
            def key(self, *args):
                self.keys.append(args)
                return subprocess.CompletedProcess(args, 0)
        game = Game()
        session = scenario.FrameSession(game, lambda raw: {'world': 20})
        self.assertEqual(session.input('ControlLeft', 'press'), 0)
        session.advance_to(4)
        self.assertEqual(session.snapshot(), {'world': 20, 'frame': 4})
        self.assertEqual(session.input('ControlLeft', 'release'), 4)
        self.assertEqual(game.keys, [('keydown', 'Control_L'), ('keyup', 'Control_L')])
        with self.assertRaisesRegex(ValueError, 'already passed'):
            session.advance_to(3)
        with self.assertRaisesRegex(ValueError, 'unsupported original input'):
            session.input('unknown', 'press')


if __name__ == '__main__':
    unittest.main()
