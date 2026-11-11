#!/usr/bin/env python3
"""Mutation controls for public animation/attachment continuation observations."""
from copy import deepcopy
import unittest
from native_session_assertions import assert_observation, assert_saved_pose, assert_dormant_preserved


def observation():
    pose = {'model': 'ACTOR', 'frame': 7}
    attachment = {'source': 'fixture.glb', 'transform': {'translation': [1, 2, 3]},
                  'global_transform': [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 1, 2, 3, 1],
                  'hidden': False}
    return {'frame': 8, 'game_seconds': 0.5, 'rendered_actor': {
        'world': 3, 'actor': {'generation': 2, 'index': 0}, 'name': 'fixture', 'owner': 10,
        'logical_pose': {'animation': pose.copy()}, 'displayed_pose': pose,
        'model': {'model': 'ACTOR', 'frame': 7, 'source': {'digest': [1]}},
        'expected_attachments': [attachment],
        'attachments': [dict(attachment, entity=11, owner=10)],
    }}


def snapshot():
    return {'logical': {'registry': {'worlds': [{'world': 3, 'generation': 1, 'actors': [{
        'name': 'fixture', 'animation': {'frame': 2},
        'model': {'current': {'animation': ['ACTOR', 2]}, 'weapon': {'model': 'SWORD'}},
    }]}]}}}


class Continuation(unittest.TestCase):
    def test_current_frame_must_agree_while_saved_animation_can_advance(self):
        assert_saved_pose(snapshot(), observation())
        broken = observation()
        broken['rendered_actor']['displayed_pose']['frame'] = 6
        with self.assertRaisesRegex(ValueError, 'logical model frame'):
            assert_observation(broken)

    def test_stale_owner_source_visibility_and_world_pose_are_rejected(self):
        for key, value in [('owner', 9), ('source', 'stale.glb'), ('hidden', True),
                           ('transform', {}), ('global_transform', [0] * 16), ('global_transform', [])]:
            with self.subTest(key=key):
                broken = observation()
                broken['rendered_actor']['attachments'][0][key] = value
                with self.assertRaises(ValueError):
                    assert_observation(broken)

    def test_missing_attachment_or_model_is_not_continuation(self):
        for key, value in [('attachments', []), ('model', None)]:
            broken = observation()
            broken['rendered_actor'][key] = value
            with self.assertRaises(ValueError):
                assert_observation(broken)

    def test_dormant_pose_is_unchanged_except_generation(self):
        before = snapshot()
        after = deepcopy(before)
        before['logical']['registry']['worlds'][0]['actors'][0]['model']['support_actor'] = [1, 4]
        after = deepcopy(before)
        after['logical']['registry']['worlds'][0]['generation'] = 6
        after['logical']['registry']['worlds'][0]['actors'][0]['model']['support_actor'] = [6, 4]
        assert_dormant_preserved(before, after, 3)
        after['logical']['registry']['worlds'][0]['actors'][0]['model']['support_actor'][0] = 5
        with self.assertRaisesRegex(ValueError, 'stale generation'):
            assert_dormant_preserved(before, after, 3)
        after['logical']['registry']['worlds'][0]['actors'][0]['model']['support_actor'][0] = 6
        after['logical']['registry']['worlds'][0]['actors'][0]['animation']['frame'] += 1
        with self.assertRaisesRegex(ValueError, 'dormant world changed'):
            assert_dormant_preserved(before, after, 3)


if __name__ == '__main__':
    unittest.main()
