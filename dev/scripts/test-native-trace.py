#!/usr/bin/env python3
"""Public trace samples bind selected-player health and pose to one update."""
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('trace', Path(__file__).with_name('native-trace.py'))
trace = importlib.util.module_from_spec(spec)
spec.loader.exec_module(trace)


class AtomicTrace(unittest.TestCase):
    def boundary(self):
        return dict(after_updates=0, update=17, generation=3, game_seconds=2.5,
            player=dict(actors=[dict(name='CYRUS', source='player',
                control=dict(falling_state=1, queued_motion=0, sliding_state=0, living_state=0),
                attributes=[0,0,0,90], animation=dict(current_group=73),
                spatial=dict(effective=dict(position=[256,512,768])))]))

    def test_loaded_world_waits_for_first_ordinary_update_before_placement(self):
        state = dict(state='InGame', scene_ready=True, scripts=dict(state='Loaded', ordinary_updates=0))
        ready = dict(state, scripts=dict(state='Loaded', ordinary_updates=1))
        with patch.object(trace, 'call', side_effect=[state, ready]) as call, \
             patch.object(trace.time, 'sleep') as sleep:
            trace.wait_live('unused', 10)
        self.assertEqual(call.call_count, 2)
        sleep.assert_called_once_with(3)

    def test_health_pose_and_clock_are_from_one_public_boundary(self):
        def call(addr, cmd):
            self.assertEqual(cmd,dict(type='schedule_input',events=[dict(after_updates=0)]))
            return dict(boundaries=[self.boundary()])
        with patch.object(trace,'call',side_effect=call):
            row=trace.sample('unused','edge')
        self.assertEqual((row['frame'],row['generation'],row['g'],row['health'],row['pos']),
                         (17,3,2.5,90,[1,2,3]))

    def test_continuation_requests_the_next_update_instead_of_resampling_current(self):
        boundary = self.boundary()
        boundary.update(after_updates=1, update=18, game_seconds=3.5)
        with patch.object(trace, 'call', return_value=dict(boundaries=[self.boundary(), boundary])) as call:
            row = trace.sample('unused', 'continuation', after_updates=1)
        self.assertEqual(call.call_args.args[1],
                         dict(type='schedule_input', events=[dict(after_updates=0), dict(after_updates=1)]))
        self.assertEqual(row['frame'], 18)
        boundary['after_updates'] = 0
        with patch.object(trace, 'call', return_value=dict(boundaries=[self.boundary(), boundary])):
            with self.assertRaisesRegex(ValueError, 'inconsistent'):
                trace.sample('unused', 'continuation', after_updates=1)

    def test_fast_control_replies_do_not_resample_the_placement_update(self):
        import copy
        import tempfile
        import sys
        boundary = self.boundary()
        def call(addr, command):
            if command['type'] != 'schedule_input':
                return {}
            self.assertEqual(command['events'][0]['after_updates'], 0)
            results = []
            for event in command['events']:
                offset = event['after_updates']
                if offset:
                    boundary['update'] += offset
                    boundary['game_seconds'] += offset
                result = copy.deepcopy(boundary)
                result['after_updates'] = offset
                results.append(result)
            return dict(boundaries=results)
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'trace.jsonl'
            with patch.object(sys, 'argv', ['native-trace', '--out', str(output),
                    '--place', '1,2,3', '--seconds', '2', '--keep-cheats']), \
                 patch.object(trace, 'wait_live'), patch.object(trace, 'call', side_effect=call), \
                 patch.object(trace.time, 'sleep'):
                trace.main()
            import json
            rows = [json.loads(line) for line in output.read_text().splitlines()]
        self.assertEqual([row['frame'] for row in rows], [17, 18, 19])
        self.assertEqual([row.get('missed_updates') for row in rows[1:]], [0, 0])

    def test_unavailable_or_wrong_boundary_never_becomes_a_sample(self):
        for change in ('missing','duplicate','offset','input','actor','clock','failure'):
            boundary=self.boundary();rows=[boundary]
            if change=='missing':rows=[]
            elif change=='duplicate':rows.append(self.boundary())
            elif change=='offset':boundary['after_updates']=1
            elif change=='input':boundary.update(key='Space',action='press')
            elif change=='actor':boundary['player']['actors']=[]
            elif change=='clock':boundary['game_seconds']=float('nan')
            else:boundary['player']['actors'][0]['failure']='edge geometry unavailable'
            with self.subTest(change=change),patch.object(trace,'call',return_value=dict(boundaries=rows)):
                with self.assertRaises((ValueError,RuntimeError)):
                    trace.sample('unused','edge')


    def test_fatal_continuation_keeps_health_and_fall_transition_on_its_update(self):
        import copy
        falling=self.boundary()
        falling['player']['actors'][0]['attributes'][3]=100
        fatal=copy.deepcopy(falling)
        fatal.update(update=18,game_seconds=2.75)
        actor=fatal['player']['actors'][0]
        actor['attributes'][3]=0
        actor['control']['falling_state']=0
        actor['animation']['current_group']=74
        with patch.object(trace,'call',side_effect=[dict(boundaries=[falling]),dict(boundaries=[fatal])]):
            previous=trace.sample('unused','edge')
            current=trace.sample('unused','fatal')
        trace.validate_continuation(previous,current)
        self.assertEqual((previous['frame'],previous['health'],previous['falling']),(17,100,1))
        self.assertEqual((current['frame'],current['health'],current['falling'],current['group']),(18,0,0,74))
        for changed in (dict(current,generation=4),dict(current,frame=17),dict(current,g=2.5)):
            with self.assertRaisesRegex(ValueError,'sequence'):
                trace.validate_continuation(previous,changed)


    def test_public_trace_releases_held_input_after_boundary_failure(self):
        import tempfile
        import sys
        with tempfile.TemporaryDirectory() as directory:
            rows=[dict(g=0,frame=1,generation=1),dict(g=1,frame=2,generation=1)]
            with patch.object(sys,'argv',['native-trace','--out',str(Path(directory)/'trace.jsonl'),
                    '--place','1,2,3','--key','ArrowUp:0:10','--seconds','3','--keep-cheats']), \
                 patch.object(trace,'wait_live'),patch.object(trace,'call') as calls, \
                 patch.object(trace,'sample',side_effect=[*rows,RuntimeError('edge failure')]), \
                 patch.object(trace.time,'sleep'):
                with self.assertRaisesRegex(RuntimeError,'edge failure'):
                    trace.main()
            inputs=[call.args[1] for call in calls.call_args_list if call.args[1]['type']=='input']
            self.assertEqual(inputs,[dict(type='input',key='ArrowUp',action='press'),
                                     dict(type='input',key='ArrowUp',action='release')])


if __name__=='__main__':
    unittest.main()
