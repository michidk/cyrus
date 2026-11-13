#!/usr/bin/env python3
"""Completion receipts survive resets without accepting interrupted work."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('stage', Path(__file__).with_name('acceptance-stage.py'))
stage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage)


class AcceptanceStage(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / 'source'
        self.source.write_text('source')
        self.artifact = self.root / 'artifact'
        self.count = self.root / 'count'
        self.directory = self.root / 'receipt'
        self.command = [sys.executable, '-c',
                        'from pathlib import Path; '
                        f'p=Path({str(self.count)!r}); '
                        'p.write_text(str(int(p.read_text())+1) if p.exists() else "1"); '
                        f'Path({str(self.artifact)!r}).write_text("complete")']

    def run_stage(self, command=None):
        return stage.run(self.directory, command or self.command, [self.source],
                         ['CYRUS_STAGE_TEST'], [self.artifact])

    def receipt(self):
        return json.loads((self.directory / 'receipt.json').read_text())

    def test_completed_receipt_reuses_unchanged_work_after_process_loss(self):
        self.assertEqual(self.run_stage(), 0)
        self.assertEqual(self.receipt()['status'], 'accepted')
        self.assertEqual(self.run_stage(), 0)
        self.assertEqual(self.count.read_text(), '1')

    def test_running_receipt_and_partial_output_never_establish_completion(self):
        self.run_stage()
        record = self.receipt()
        record['status'] = 'running'
        stage.save(self.directory / 'receipt.json', record)
        self.assertEqual(self.run_stage(), 0)
        self.assertEqual(self.count.read_text(), '2')

    def test_nonzero_exit_and_missing_output_are_rejected(self):
        for command in ([sys.executable, '-c', 'raise SystemExit(7)'],
                        [sys.executable, '-c', 'pass']):
            self.assertNotEqual(self.run_stage(command), 0)
            self.assertEqual(self.receipt()['status'], 'rejected')

    def test_changed_source_environment_or_command_requires_execution(self):
        self.run_stage()
        self.source.write_text('changed')
        self.run_stage()
        with patch.dict(os.environ, CYRUS_STAGE_TEST='changed'):
            self.run_stage()
            self.run_stage(self.command + ['changed'])
        self.assertEqual(self.count.read_text(), '4')

    def test_corrupt_log_or_artifact_requires_execution(self):
        self.run_stage()
        (self.directory / 'output.log').write_text('truncated')
        self.run_stage()
        self.artifact.write_text('truncated')
        self.run_stage()
        self.artifact.unlink()
        self.run_stage()
        self.assertEqual(self.count.read_text(), '4')

    def test_source_mutation_during_execution_cannot_pass(self):
        command = self.command[:]
        command[2] += f'; Path({str(self.source)!r}).write_text("changed")'
        self.assertNotEqual(self.run_stage(command), 0)
        self.assertEqual(self.receipt()['status'], 'rejected')

    def test_public_command_line_retains_and_reuses_artifact(self):
        command = [sys.executable, str(Path(stage.__file__)), str(self.directory),
                   '--input', str(self.source), '--output', str(self.artifact),
                   '--command', *self.command]
        for _ in range(2):
            stage.subprocess.run(command, check=True)
        self.assertEqual(self.receipt()['status'], 'accepted')
        self.assertEqual(self.count.read_text(), '1')

    def test_interrupted_child_leaves_running_receipt_for_retry(self):
        with patch.object(stage.subprocess, 'run', side_effect=OSError('reset')):
            with self.assertRaises(OSError):
                self.run_stage()
        self.assertEqual(self.receipt()['status'], 'running')
        self.assertEqual(self.run_stage(), 0)


if __name__ == '__main__':
    unittest.main()
