"""Validated checkpoint contracts for normalized paired-replay observations."""
import math


FIELDS = {'world', 'flags', 'globals', 'inventory', 'position_q8', 'health',
          'opponent', 'target_health', 'actor_health', 'separation_l1_q8'}


def integer(value):
    return type(value) is int


def position(value):
    return isinstance(value, list) and len(value) == 3 and all(integer(v) for v in value)


def validate_checkpoint(expected):
    """Reject unsupported or malformed contracts before either backend opens."""
    if not isinstance(expected, dict):
        raise ValueError('paired checkpoint must be an object')
    unknown = expected.keys() - FIELDS
    if unknown:
        raise ValueError(f'unsupported paired checkpoint fields: {", ".join(sorted(unknown))}')
    for field, value in expected.items():
        valid = True
        if field in ('world', 'health', 'target_health'):
            valid = integer(value) and (field != 'target_health' or value >= 0)
        elif field == 'opponent':
            valid = isinstance(value, str) and bool(value.strip())
        elif field == 'actor_health':
            valid = (isinstance(value, dict) and bool(value)
                     and all(isinstance(name, str) and bool(name.strip()) and integer(health)
                             for name, health in value.items()))
        elif field == 'separation_l1_q8':
            valid = (isinstance(value, dict) and set(value) == {'opponent', 'value'}
                     and isinstance(value['opponent'], str) and bool(value['opponent'].strip())
                     and integer(value['value']) and value['value'] >= 0)
        elif field == 'position_q8':
            valid = (isinstance(value, dict) and set(value) == {'value', 'tolerance'}
                     and position(value['value'])
                     and type(value['tolerance']) in (int, float)
                     and math.isfinite(value['tolerance']) and value['tolerance'] >= 0)
        elif field in ('flags', 'globals', 'inventory'):
            valid = isinstance(value, dict) and bool(value)
            if valid:
                for name, item in value.items():
                    if not isinstance(name, str) or not name.strip():
                        valid = False
                        break
                    if field == 'flags':
                        valid = (isinstance(item, dict) and bool(item)
                                 and not item.keys() - {'value', 'latched'}
                                 and all(type(v) is (bool if k == 'latched' else int)
                                         for k, v in item.items()))
                    else:
                        valid = integer(item)
                    if not valid:
                        break
        if not valid:
            raise ValueError(f'unsupported or malformed paired checkpoint {field}')
    if ('opponent' in expected) != ('target_health' in expected):
        raise ValueError('paired checkpoint opponent and target_health are required together')


def lookup(state, *path):
    for key in path:
        if not isinstance(state, dict) or key not in state:
            return None
        state = state[key]
    return state


def evaluate_checkpoint(expected, measured, baseline):
    """Retain expected and measured values; absent measurements never count as success.

    Route poses use the route runner's inclusive Euclidean Q8 tolerance. A hit
    checkpoint requires a living player and actual opponent health loss since
    replay entry, in addition to reaching the target health.
    """
    rows = []

    def compare(field, wanted, actual, available, matches, **evidence):
        rows.append(dict(field=field, expected=wanted, actual=actual,
                         status=('unavailable' if not available else
                                 'match' if matches else 'different'), **evidence))

    for field, wanted in expected.items():
        if field in ('opponent', 'target_health'):
            continue
        if field in ('world', 'health'):
            actual = lookup(measured, 'player', 'health') if field == 'health' else measured.get(field)
            compare(field, wanted, actual, integer(actual), actual == wanted)
        elif field in ('flags', 'globals', 'inventory'):
            for name, value in wanted.items():
                parts = value.items() if field == 'flags' else [(None, value)]
                for part, target in parts:
                    path = (field, name) if part is None else (field, name, part)
                    actual = lookup(measured, *path)
                    compare('.'.join(path), target, actual, type(actual) is type(target), actual == target)
        elif field == 'actor_health':
            for name, health in wanted.items():
                actual = lookup(measured, 'actors', name, 'health')
                compare(f'actors.{name}.health', health, actual, integer(actual), actual == health)
        elif field == 'separation_l1_q8':
            player = lookup(measured, 'player', 'position_q8')
            opponent = lookup(measured, 'actors', wanted['opponent'], 'position_q8')
            actual = (sum(abs(a - b) for a, b in zip(player, opponent))
                      if position(player) and position(opponent) else None)
            compare(f"separation_l1_q8.{wanted['opponent']}", wanted['value'], actual,
                    actual is not None, actual == wanted['value'])
        elif field == 'position_q8':
            actual = lookup(measured, 'player', 'position_q8')
            distance = math.dist(wanted['value'], actual) if position(actual) else None
            compare(field, wanted['value'], actual, distance is not None,
                    distance is not None and distance <= wanted['tolerance'],
                    distance_q8=distance, tolerance=wanted['tolerance'])
    if 'opponent' in expected:
        name, target = expected['opponent'], expected['target_health']
        actual = lookup(measured, 'actors', name, 'health')
        initial = lookup(baseline, 'actors', name, 'health')
        compare(f'actors.{name}.health', target, actual, integer(actual),
                integer(actual) and actual <= target)
        decreased = actual < initial if integer(actual) and integer(initial) else None
        compare(f'actors.{name}.health_decreased', True, decreased, decreased is not None,
                decreased is True, initial_health=initial, health=actual)
        health = lookup(measured, 'player', 'health')
        compare('player.alive', True, health > 0 if integer(health) else None,
                integer(health), integer(health) and health > 0, health=health)
    return rows
