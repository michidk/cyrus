#!/usr/bin/env python3
"""Verify public native light restoration, rejection continuity and world retirement."""
import argparse
import fcntl
import importlib.util
import json
from pathlib import Path

spec = importlib.util.spec_from_file_location('session_capture', Path(__file__).with_name('native-session-capture.py'))
capture = importlib.util.module_from_spec(spec)
spec.loader.exec_module(capture)


def changed_eligibility(data, actor_index):
    """Change only current-world placement eligibility, retaining valid framing."""
    try:
        from blake3 import blake3
    except ImportError as error:
        raise RuntimeError('install Python blake3 to encode the rejection control') from error
    snapshot = capture.document(data.decode())
    registry = snapshot['logical']['registry']
    world = next(world for world in registry['worlds'] if world['world'] == registry['current'])
    actor = world['actors'][actor_index]
    if not (actor.get('light') or {}).get('enabled') or not isinstance(actor.get('map_light'), bool):
        raise ValueError('select an enabled installed light with current-schema placement eligibility')
    actor['map_light'] = not actor['map_light']
    payload = json.dumps(snapshot, separators=(',', ':')).encode()
    return b'CYRUS-SESSION 1\n' + blake3(payload).hexdigest().encode() + b'\n' + payload



def saved_light(data, actor_index, world_id=None):
    """Read the selected installed light owner from a public session document."""
    registry = capture.document(data.decode())['logical']['registry']
    world_id = registry['current'] if world_id is None else world_id
    world = next(world for world in registry['worlds'] if world['world'] == world_id)
    actor = world['actors'][actor_index]
    light = actor.get('light')
    if not light or not light['enabled'] or not light['locked']:
        raise ValueError('fixture requires an active locked scripted light')
    return {'world': world_id, 'name': actor['name'], 'map_light': actor['map_light'], 'light': light}


def assert_saved_light(before, after):
    """This fixture selects steady installed state; a changed owner or state fails."""
    if before != after:
        raise ValueError('saved scripted light owner/state changed after restoration')


def assert_bound(before, after, replaced):
    """Require actual material rows and source ownership, not only load completion."""
    old, new = (record['bound_light_image'] for record in (before, after))
    if not old['rows_after_lookup_256']:
        raise ValueError('fixture view must contain an active rendered light')
    for field in ('world', 'rows_after_lookup_256', 'dimensions', 'format'):
        if old[field] != new[field]:
            raise ValueError(f'restored bound light {field} changed')
    if (old['generation'] != new['generation']) != replaced:
        raise ValueError('bound light generation does not match publication outcome')
    if replaced and old['entity'] == new['entity']:
        raise ValueError('restoration reused a retired scene mesh owner')
    if not replaced and any(old[field] != new[field] for field in ('entity', 'material', 'image')):
        raise ValueError('rejected restoration replaced a live material binding')


class LightDriver(capture.ContinuationDriver):
    def load(self, name, runtime_path):
        # Keep unrelated gameplay UI transitions out of slow asynchronous preparation.
        # The public console suspends simulation; every continuation check runs after closing it.
        self.request('console', open=True)
        try:
            super().load(name, runtime_path, prepare_seconds=self.args.prepare_seconds)
        finally:
            self.request('console', open=False)

    def acknowledge_rejection(self):
        try:
            self.request('session_load_result', acknowledge=True)
        except RuntimeError as error:
            # Acknowledgement consumes the terminal result and returns its original error.
            if 'map-light eligibility differs from installed placement' not in str(error):
                raise

    def observe(self):
        return self.request('bound_light_image', name=self.args.node, primitive=self.args.primitive)

    def view(self):
        def positioned():
            self.request('flycam', enabled=True, position=self.args.position)
            state = self.progress(self.ready(self.args.scene.upper()))
            position = state.get('flycam', {}).get('position')
            if position is None or any(abs(actual - expected) > .001
                                       for actual, expected in zip(position, self.args.position)):
                return None
            return self.observe()
        # Entry framing may still run after generic scene readiness. Establish the
        # requested inspection view before capturing; never retry light assertions.
        return self.wait(positioned, self.args.seconds)

    def run(self):
        state = self.ready()
        if self.args.runtime_seed:
            self.load('bootstrap-light-fixture', self.args.runtime_seed)
            self.progress(self.ready(self.args.scene.upper()))
        else:
            for scene in (self.args.dormant, self.args.scene):
                self.phase('visit-' + scene)
                if state.get('scene', '').upper() != scene.upper():
                    self.request('load_scene', name=scene)
                state = self.progress(self.ready(scene.upper()))
        before = self.view()
        assert_bound(before, before, replaced=False)
        data = self.capture()
        logical_before = saved_light(data, self.args.light_actor)
        # Save the public capture and exact negative document before publication.
        capture.atomic(self.output / 'before.session', data)
        capture.atomic(self.output / 'ineligible.session', changed_eligibility(data, self.args.light_actor))
        capture.atomic(self.output / 'before.json', json.dumps(before, indent=2).encode())
        self.load('publish-light-fixture', str(Path(self.args.runtime_output) / 'before.session'))
        first = self.observe()
        capture.atomic(self.output / 'published.json', json.dumps(first, indent=2).encode())
        assert_bound(before, first, replaced=True)
        self.progress(self.ready())
        logical_published = saved_light(self.capture(), self.args.light_actor)
        assert_saved_light(logical_before, logical_published)
        rejected_before = self.observe()
        try:
            self.load('reject-ineligible-light', str(Path(self.args.runtime_output) / 'ineligible.session'))
        except RuntimeError as error:
            if 'map-light eligibility differs from installed placement' not in str(error):
                raise
            rejection = str(error)
        else:
            raise ValueError('inconsistent installed light eligibility was published')
        rejected_after = self.observe()
        self.acknowledge_rejection()
        assert_saved_light(logical_before, saved_light(self.capture(), self.args.light_actor))
        assert_bound(rejected_before, rejected_after, replaced=False)
        capture.atomic(self.output / 'rejected.json', json.dumps({
            'error': rejection, 'before': rejected_before, 'after': rejected_after,
        }, indent=2).encode())
        self.phase('retire-light-world')
        self.request('load_scene', name=self.args.dormant)
        self.progress(self.ready(self.args.dormant.upper()))
        try:
            self.observe()
        except RuntimeError as error:
            if 'absent' not in str(error):
                raise
        else:
            raise ValueError('retired world mesh still supplies a live light binding')
        assert_saved_light(logical_before, saved_light(self.capture(), self.args.light_actor, logical_before['world']))
        self.phase('revisit-light-world')
        self.request('load_scene', name=self.args.scene)
        self.progress(self.ready(self.args.scene.upper()))
        revisited = self.view()
        assert_bound(first, revisited, replaced=True)
        capture.atomic(self.output / 'revisited.json', json.dumps(revisited, indent=2).encode())
        logical_revisited = saved_light(self.capture(), self.args.light_actor)
        assert_saved_light(logical_before, logical_revisited)
        capture.atomic(self.output / 'logical.json', json.dumps({'before': logical_before, 'published': logical_published, 'revisited': logical_revisited}, indent=2).encode())
        self.screenshot('light-revisit.png')
        capture.atomic(self.output / 'result.json', json.dumps({
            'publication': True, 'rejected_restore_preserved_output': True,
            'retired_world_binding_removed': True, 'dormant_revisit': True,
            'binary_sha256': capture.file_digest(self.args.binary),
        }, indent=2).encode())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('port', type=int)
    parser.add_argument('output', type=Path)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--runtime-output', required=True)
    parser.add_argument('--runtime-seed', help='optional compatible seed; otherwise visit both worlds')
    parser.add_argument('--scene', required=True)
    parser.add_argument('--dormant', required=True)
    parser.add_argument('--light-actor', type=int, required=True)
    parser.add_argument('--node', required=True)
    parser.add_argument('--primitive', type=int, default=0)
    parser.add_argument('--position', nargs=3, type=float, required=True)
    parser.add_argument('--seconds', type=float, default=180)
    parser.add_argument('--prepare-seconds', type=float, default=600)
    parser.add_argument('--game-seconds', type=float, default=.3)
    parser.add_argument('--interval', type=float, default=.1)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    with (args.output / 'capture.lock').open('w') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        (args.output / 'result.json').unlink(missing_ok=True)
        (args.output / 'failure.json').unlink(missing_ok=True)
        try:
            LightDriver(args).run()
        except Exception as error:
            capture.atomic(args.output / 'failure.json', json.dumps({
                'error': str(error), 'type': type(error).__name__,
            }).encode())
            raise


if __name__ == '__main__':
    main()
