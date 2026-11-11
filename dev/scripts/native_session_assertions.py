"""Assertions over public session documents and post-update actor observations."""
import math


def assert_observation(record):
    actor = record['rendered_actor']
    pose = actor['displayed_pose']
    if not pose or actor['logical_pose']['animation'] != pose:
        raise ValueError('displayed animation differs from retained logical model frame')
    model = actor['model']
    if not model or (model['model'], model['frame']) != (pose['model'], pose['frame']):
        raise ValueError('rendered model source/frame owner differs from displayed animation')
    expected = sorted(actor['expected_attachments'], key=lambda item: item['source'])
    actual = sorted(actor['attachments'], key=lambda item: item['source'])
    if not expected or len(expected) != len(actual):
        raise ValueError('active attachment census differs from retained logical geometry')
    for want, root in zip(expected, actual):
        if root['owner'] != actor['owner']:
            raise ValueError('attachment belongs to a different rendered actor')
        for key in ('source', 'transform', 'hidden'):
            if root[key] != want[key]:
                raise ValueError(f'attachment {key} differs from retained logical pose')
        # Only numerical float32 transform-propagation rounding is tolerated;
        # this is an observation tolerance, not a game timing/geometry rule.
        if (root['global_transform'] is None or len(root['global_transform']) != 16
                or len(want['global_transform']) != 16 or not all(
                math.isclose(a, b, rel_tol=2**-20, abs_tol=2**-20)
                for a, b in zip(root['global_transform'], want['global_transform']))):
            raise ValueError('attachment world matrix differs from its scene-space pose')
    return actor


def saved_actor(snapshot, world_id, index):
    world = next(world for world in snapshot['logical']['registry']['worlds'] if world['world'] == world_id)
    actor = world['actors'][index]
    if not actor['animation'] or not actor['model']['current'] or not actor['model']['weapon']:
        raise ValueError('fixture actor needs active animation and an installed equipped attachment')
    return actor


def assert_saved_pose(snapshot, observation):
    observed = assert_observation(observation)
    actor = saved_actor(snapshot, observed['world'], observed['actor']['index'])
    if actor['name'] != observed['name']:
        raise ValueError('restored animation belongs to a different logical actor')
    # Frames may advance at the first update. The current observation above must
    # agree exactly with that update's logical frame, not with an older frame.
    if actor['model']['current']['animation'][0] != observed['displayed_pose']['model']:
        raise ValueError('restored animation model differs from the selected installed fixture')
    return actor


def assert_dormant_preserved(before, after, world_id):
    def normalized(snapshot):
        world = next(world for world in snapshot['logical']['registry']['worlds'] if world['world'] == world_id)
        def normalize(value):
            if isinstance(value, dict):
                return {key: normalize(item) for key, item in value.items() if key != 'generation'}
            if isinstance(value, list):
                return [normalize(item) for item in value]
            return value
        # Actor IDs are serialized as [generation, index] at these schema
        # fields. Normalize only their generation, retaining the target index.
        from copy import deepcopy
        world = deepcopy(world)
        def actor_id(value):
            if value is not None:
                if value[0] != world['generation']:
                    raise ValueError('dormant actor reference uses a stale generation')
                value[0] = 0
        for actor in world['actors']:
            actor_id(actor.get('model', {}).get('support_actor'))
            combat = (actor.get('control') or {}).get('combat')
            if combat:
                actor_id(combat.get('target'))
        lists = world.get('actor_lists') or {}
        for name in ('selected', 'separate_selected'):
            for value in lists.get(name, []):
                actor_id(value)
        camera = world.get('camera') or {}
        for name in ('alternate_actor', 'camera_actor'):
            actor_id(camera.get(name))
        for record in camera.get('records', []):
            actor_id(record.get('actor'))
        return normalize(world)
    if normalized(before) != normalized(after):
        raise ValueError('dormant world changed before its revisit (excluding generation rebinding)')
