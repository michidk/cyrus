#!/usr/bin/env python3
"""Restart controls for host-mounted runtime acceptance receipts."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('probe', Path(__file__).with_name('probe-checkpoint.py'))
probe = importlib.util.module_from_spec(spec)
spec.loader.exec_module(probe)


class ProbeCheckpoint(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.path = Path(self.temp.name) / 'run.json'
        self.run = probe.Run.start(self.path, 'container-id', {'scene': 'TEST', 'target_scale': 1024})

    def ready(self):
        self.run.readiness({'readiness': 'selected', 'frame': 3, 'selected_count': 1})

    def boundary(self, update=10, scale=0, step=256):
        self.run.boundary('climb', 7, update, scale, step, {'living': 11})

    def test_readiness_is_durable_before_input_and_cannot_admit_absent_actor(self):
        self.run.readiness({'readiness': 'actor_absent', 'frame': 0, 'selected_count': 0})
        record = json.loads(self.path.read_text())
        self.assertEqual(record['readiness']['readiness'], 'actor_absent')
        with self.assertRaisesRegex(ValueError, 'selected actor'):
            self.boundary()
        self.ready()
        self.boundary()
        self.assertEqual(json.loads(self.path.read_text())['last']['update'], 10)

    def test_lost_container_is_explicit_and_new_attempt_never_joins_old_clock(self):
        self.ready(); self.boundary(); self.boundary(11, 256)
        with patch.object(probe, 'container_alive', return_value=False):
            self.assertEqual(probe.inspect_run(self.path)['status'], 'lost')
        old = self.run.record['attempt']
        resumed = probe.Run.start(self.path, 'new-container', self.run.record['fixture'], restart=True)
        self.assertNotEqual(resumed.record['attempt'], old)
        self.assertNotIn('last', resumed.record)
        self.assertEqual(len(list(self.path.parent.glob('run.*.json'))), 1)
        with self.assertRaisesRegex(ValueError, 'selected actor'):
            resumed.boundary('climb', 8, 1, 0, 256, {})

    def test_live_attempt_requires_attach_and_cannot_be_overwritten(self):
        with self.assertRaisesRegex(ValueError, 'inspect'):
            probe.Run.start(self.path, 'container-id', self.run.record['fixture'])
        with patch.object(probe, 'container_alive', return_value=True):
            with self.assertRaisesRegex(ValueError, 'live'):
                probe.Run.start(self.path, 'container-id', self.run.record['fixture'], restart=True)

    def test_duplicate_gap_wrong_generation_and_scale_mismatch_are_rejected(self):
        self.ready(); self.boundary()
        for generation, update, scale in [(7,10,256), (7,12,256), (8,11,256), (7,11,512)]:
            with self.subTest(generation=generation, update=update, scale=scale):
                with self.assertRaises(ValueError):
                    self.run.boundary('climb', generation, update, scale, 256, {})
        self.assertEqual(json.loads(self.path.read_text())['last']['update'], 10)

    def test_acceptance_requires_exact_target_and_retains_complete_stage_after_runtime_loss(self):
        self.ready(); self.boundary()
        with self.assertRaisesRegex(ValueError, 'target'):
            self.run.accept('climb')
        for i in range(1, 5): self.boundary(10+i, 256*i)
        self.run.accept('climb')
        with patch.object(probe, 'container_alive', return_value=False):
            record = probe.inspect_run(self.path)
        self.assertEqual(record['status'], 'accepted')
        self.assertEqual(len(record['boundaries']), 5)
        self.assertEqual(record['last']['consumed_scale'], 1024)

    def test_lost_job_in_live_container_is_not_loading(self):
        with patch.object(probe, 'container_alive', return_value=True), \
             patch.object(probe, 'job_alive', return_value=False):
            self.assertEqual(probe.inspect_run(self.path, '/evidence/job.pid')['status'], 'lost')

    def test_polling_clock_progress_cannot_complete_acceptance(self):
        self.ready()
        self.run.progress({'game_seconds': 4, 'ordinary_updates': 60})
        self.assertEqual(json.loads(self.path.read_text())['progress']['game_seconds'], 4)
        with self.assertRaisesRegex(ValueError, 'target'):
            self.run.accept('climb')
        self.assertEqual(self.run.record['boundaries'], [])

    def test_booting_control_port_does_not_abort_or_admit_input(self):
        observations = iter([ConnectionRefusedError('booting'),
            {'readiness': 'actor_absent', 'frame': 1, 'selected_count': 0},
            {'readiness': 'selected', 'frame': 3, 'selected_count': 1}])
        def observe():
            row = next(observations)
            if isinstance(row, Exception): raise row
            return row
        with patch.object(probe.time, 'sleep'):
            self.run.wait_ready(observe, 10)
        self.assertEqual(self.run.record['polls'], 3)
        self.assertEqual(self.run.record['status'], 'waiting')
        self.assertEqual(self.run.record['readiness']['readiness'], 'selected')

    def test_readiness_timeout_retains_last_failure_without_admitting_input(self):
        with patch.object(probe.time, 'monotonic', side_effect=[0, 2]):
            with self.assertRaisesRegex(TimeoutError, 'readiness'):
                self.run.wait_ready(lambda: {'readiness':'actor_absent','frame':0,'selected_count':0}, 1)
        self.assertEqual(json.loads(self.path.read_text())['readiness']['readiness'], 'actor_absent')
        with self.assertRaises(ValueError): self.boundary()

    def test_abrupt_driver_exit_leaves_readable_host_receipt(self):
        import subprocess
        code = ("import runpy,sys,os; from pathlib import Path; "
                "p=runpy.run_path(sys.argv[1]); "
                "r=p['Run'].start(Path(sys.argv[2]),'container',{'target_scale':256}); "
                "r.readiness({'readiness':'actor_absent','frame':0,'selected_count':0}); os._exit(17)")
        path = self.path.parent / 'abrupt.json'
        result = subprocess.run(['python3', '-c', code, str(Path(probe.__file__)), str(path)])
        self.assertEqual(result.returncode, 17)
        self.assertEqual(json.loads(path.read_text())['readiness']['readiness'], 'actor_absent')

    def test_process_birth_receipt_distinguishes_pid_reuse(self):
        import subprocess
        path = self.path.parent / 'job.json'
        probe.register_job(path)
        command = None
        with patch.object(probe.subprocess, 'run') as run:
            probe.job_alive('container', str(path))
            command = run.call_args.args[0][3:]
        real_run = subprocess.run
        self.assertEqual(real_run(command, capture_output=True).returncode, 0)
        job = json.loads(path.read_text());job['start_ticks'] = 'wrong'
        path.write_text(json.dumps(job))
        self.assertNotEqual(real_run(command, capture_output=True).returncode, 0)

    def test_rejected_clock_window_is_not_relabelled_as_container_loss(self):
        self.ready();self.boundary();self.boundary(11, 256, 1000)
        self.boundary(12, 1256)
        with self.assertRaisesRegex(ValueError, 'target'): self.run.accept('climb')
        self.run.reject('target 1024 skipped between 256 and 1256')
        with patch.object(probe, 'container_alive', return_value=False) as alive:
            record = probe.inspect_run(self.path)
        alive.assert_not_called()
        self.assertEqual(record['status'], 'rejected')
        self.assertIn('1024 skipped', record['reason'])

    def test_adaptive_targets_accept_only_an_observed_member_and_terminal_unknown_step(self):
        path = self.path.parent / 'adaptive.json'
        self.run = probe.Run.start(path, 'container', {'target_scales':[1024,1334]})
        self.ready()
        self.boundary(10,0,512);self.boundary(11,512,438)
        self.boundary(12,950,384)
        with self.assertRaisesRegex(ValueError, 'target'):self.run.accept('climb')
        self.boundary(13,1334,None)
        with self.assertRaisesRegex(ValueError, 'gap'):
            self.boundary(14,1500,256)
        self.run.accept('climb')
        self.assertEqual(json.loads(path.read_text())['accepted_target_scale'],1334)
        self.assertIsNone(self.run.record['last']['next_scale'])

    def test_adaptive_targets_are_bounded_exact_and_cannot_be_ambiguous(self):
        for fixture in ({'target_scales':[]},{'target_scales':[0]},
                        {'target_scales':[512,512]},{'target_scales':[512.0]},
                        {'target_scale':512,'target_scales':[512]},
                        {'target_scales':list(range(1,258))}):
            with self.assertRaises(ValueError):
                probe.Run.start(self.path.parent/'invalid.json','container',fixture)

    def test_resume_rejects_changed_fixture(self):
        with patch.object(probe, 'container_alive', return_value=False):
            probe.inspect_run(self.path)
        with self.assertRaisesRegex(ValueError, 'fixture'):
            probe.Run.start(self.path, 'new', {'scene': 'OTHER', 'target_scale': 1024}, restart=True)


if __name__ == '__main__':
    unittest.main()
