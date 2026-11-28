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


def assert_arrival(before, after, observations, expected):
    """Require selected, visible arrival owners and settled post-restore dock frames.

    Static scenery has no replacement animation root. Selection and saved owner
    visibility establish its draw eligibility, not pixel visibility or frustum parity.
    Animated observations may advance beyond the capture, but their logical and
    displayed frames and settled mesh owner must agree at the observed update.
    """
    world_id = expected['world']
    roster = expected['actors']
    if (not roster or any(type(a['index']) is not int or a['index'] < 0
                          or not a['name'] or type(a['animated']) is not bool for a in roster)
            or len({a['index'] for a in roster}) != len(roster)):
        raise ValueError('arrival expectations need distinct nonnegative actor indices, names and animation kinds')

    def check(snapshot):
        registry = snapshot['logical']['registry']
        worlds = [w for w in registry['worlds'] if w['world'] == world_id]
        if registry['current'] != world_id or len(worlds) != 1:
            raise ValueError('arrival current world is absent or ambiguous')
        world = worlds[0]
        generation = world['generation']
        lists = world['actor_lists']
        if not lists or not lists['traversal_prepared']:
            raise ValueError('arrival requires a prepared draw selection')
        for actor in world['actors']:
            runtime = actor.get('runtime')
            if (actor.get('animation_failure') is not None or actor.get('update_failure') is not None
                    or runtime and runtime['state']['failure'] is not None):
                raise ValueError(f"arrival actor {actor['name']}: script/animation failure")
        for want in roster:
            index, name = want['index'], want['name']
            actor = world['actors'][index]
            if actor['name'] != name:
                raise ValueError(f'{name}: installed actor identity differs')
            if [generation, index] not in lists['selected']:
                raise ValueError(f'{name}: absent from cached draw selection')
            if actor['control']['hidden'] or actor['control']['invisible']:
                raise ValueError(f'{name}: logical owner is hidden')
            owners = [b for b in snapshot['entity_bindings'] if b['actor'] == [generation, index]]
            if len(owners) != 1 or owners[0]['visibility'] not in (0, 2):
                raise ValueError(f'{name}: visible bound owner absent or ambiguous')
            if want['animated']:
                pose = owners[0]['displayed']
                logical = actor['model']['current']['animation']
                if not pose or pose[1] <= 0 or pose != logical:
                    raise ValueError(f'{name}: captured displayed frame differs from logical animation')
        return world

    original, restored = check(before), check(after)
    if (original['generation'] == restored['generation']
            or any(original[key] != restored[key] for key in ('content_id', 'source_identity'))):
        raise ValueError('arrival requires a replacement generation of the same installed source')
    for want in roster:
        index, name = want['index'], want['name']
        matches = [r['rendered_actor'] for r in observations
                   if r['rendered_actor']['actor']['index'] == index]
        if len(matches) != 1:
            raise ValueError(f'{name}: rendered observation absent or ambiguous')
        actor = matches[0]
        if (actor['world'] != world_id or actor['name'] != name
                or actor['actor']['generation'] != restored['generation']):
            raise ValueError(f'{name}: rendered observation belongs to a different owner')
        if want['animated']:
            pose, model = actor['displayed_pose'], actor['model']
            saved = restored['actors'][index]['model']['current']['animation']
            if (not pose or pose['frame'] <= 0 or actor['logical_pose']['animation'] != pose
                    or saved[0] != pose['model'] or not model
                    or (model['model'], model['frame']) != (pose['model'], pose['frame'])
                    or model['visibility'] not in (0, 2)):
                raise ValueError(f'{name}: rendered frame owner is hidden, unsettled or differs from animation')
    return {'world': world_id, 'actors_checked': len(roster),
            'before_generation': original['generation'], 'after_generation': restored['generation']}
