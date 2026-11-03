#!/usr/bin/env python3
"""Check evidence joins and conservative status handling without Cargo or installed data."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('native-audit.py')
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location('native_audit', SCRIPT)
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class BehaviorAuditTest(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.catalogue = {'Reviewed': 'function', 'Mentioned': 'task'}
        self.used = {name: {'world.scripts'} for name in self.catalogue}
        self.row = {'native': 'Reviewed', 'status': 'partial', 'owner': 5,
                    'scope': 'Public call', 'caller': 'public::call', 'tests': [],
                    'evidence': ['https://example.test/receipt'],
                    'limits': 'Live continuation has not been observed.'}
        self.path = self.root / 'review.json'

    def save(self, rows):
        self.path.write_text(json.dumps({'natives': rows}))

    def test_source_and_test_mentions_never_promote_unaudited_behavior(self):
        self.save([self.row])
        (self.root / 'world.scripts').write_text(json.dumps({
            'definition': {'functions': [{'name': name, 'kind': kind}
                                         for name, kind in self.catalogue.items()]},
            'actors': {'ACTOR': {'program': [{'kind': 'call', 'function': name}
                                            for name in self.catalogue]}}}))
        (self.root / 'test.rs').write_text('"Mentioned"; #[cfg(test)] fn mentioned() {}')
        result = subprocess.run(['python3', str(SCRIPT), '--scripts', str(self.root),
                                 '--sources', str(self.root), '--behavior-matrix',
                                 '--behavior', str(self.path)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = {line.split('\t')[0]: line.split('\t') for line in result.stdout.splitlines()[1:]}
        self.assertEqual(rows['Reviewed'][3:5], ['partial', '#5'])
        self.assertEqual(rows['Mentioned'][3:5], ['unaudited', '-'])
        self.assertIn('Live continuation has not been observed.', rows['Reviewed'][-1])

    def test_invalid_and_incomplete_reviews_are_rejected(self):
        variants = [
            [self.row, self.row],
            [dict(self.row, native='Unknown')],
            [dict(self.row, status='implemented')],
            [dict(self.row, owner=0)],
            [dict(self.row, limits='')],
            [dict(self.row, evidence=[])],
            [dict(self.row, evidence='a source mention')],
            [dict(self.row, status='done')],
            [dict(self.row, status='done', acceptance={'native': 'only one host'})],
        ]
        for rows in variants:
            with self.subTest(rows=rows):
                self.save(rows)
                with self.assertRaises(ValueError):
                    audit.behavior_rows(self.path, self.catalogue, self.used)

    def test_done_requires_explicit_three_host_references_and_missing_is_explicit(self):
        self.save([dict(self.row, status='done', acceptance={
            'original': 'original receipt', 'native': 'native receipt', 'browser': 'browser receipt'})])
        self.assertEqual(dict(audit.behavior_rows(self.path, self.catalogue, self.used))['Reviewed']['status'], 'done')
        self.save([dict(self.row, status='missing')])
        rows = dict(audit.behavior_rows(self.path, self.catalogue, self.used))
        self.assertEqual(rows['Reviewed']['status'], 'missing')
        self.assertIsNone(rows['Mentioned'])

    def test_instruction_matrix_counts_statements_without_promoting_occurrences(self):
        self.path.write_text(json.dumps({'instructions': [dict(
            self.row, instruction='assign', native='ignored')]}))
        (self.root / 'world.scripts').write_text(json.dumps({'actors': {
            'ONE': {'program': {'steps': {
                '0': {'operation': {'type': 'assign', 'formula': {'type': 'call'}}},
                '1': {'operation': {'type': 'assign'}},
                '2': {'operation': {'type': 'task_resume'}},
            }}}}}))
        result = subprocess.run(['python3', str(SCRIPT), '--scripts', str(self.root),
                                 '--instruction-matrix', '--behavior', str(self.path)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = {line.split('\t')[0]: line.split('\t')
                for line in result.stdout.splitlines()[1:]}
        self.assertEqual(rows['assign'][1:5], ['1', '2', 'partial', '#5'])
        self.assertEqual(rows['call'][1:4], ['0', '0', 'unaudited'])
        self.assertEqual(rows['task_resume'][1:4], ['1', '1', 'unaudited'])
        self.assertEqual(rows['unsupported'][1:4], ['0', '0', 'unaudited'])

    def test_instruction_matrix_refuses_empty_input_instead_of_claiming_a_census(self):
        self.path.write_text(json.dumps({'instructions': []}))
        result = subprocess.run(['python3', str(SCRIPT), '--scripts', str(self.root),
                                 '--instruction-matrix', '--behavior', str(self.path)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn('no decoded instruction statements found', result.stderr)

    def test_instruction_review_rejects_unknown_duplicates_and_unproven_done(self):
        (self.root / 'world.scripts').write_text(json.dumps({'actors': {
            'ONE': {'program': {'steps': {'0': {'operation': {'type': 'yield'}}}}}}}))
        for rows in [
            [dict(self.row, instruction='invented')],
            [dict(self.row, instruction='assign')] * 2,
            [dict(self.row, instruction='assign', status='done')],
        ]:
            self.path.write_text(json.dumps({'instructions': rows}))
            result = subprocess.run(['python3', str(SCRIPT), '--scripts', str(self.root),
                                     '--instruction-matrix', '--behavior', str(self.path)],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 1, result.stdout)
            self.assertIn('invalid instruction audit', result.stderr)

    def test_rejections_group_causes_without_counting_fallback_cursors_as_actors(self):
        def actor(reason):
            return {'program': {'steps': {
                '0': {'operation': {'type': 'unsupported', 'reason': reason}},
                '9': {'operation': {'type': 'unsupported', 'reason': reason}},
            }}}
        (self.root / 'one.scripts').write_text(json.dumps({'actors': {
            'A': actor('script instruction 7: function Query requires 0 arguments, found 1'),
            'B': actor('script instruction 4: function Query requires 0 arguments, found 1'),
            'C': actor('unrecovered decoder cause'),
        }}))
        (self.root / 'two.scripts').write_text(json.dumps({'actors': {
            'A': actor('script instruction 8: function Query requires 0 arguments, found 1'),
        }}))
        groups = audit.rejection_rows(self.root, '*.scripts')
        group = groups[('arity', 'Query', '0', '1')]
        self.assertEqual(group['statements'], 6)
        self.assertEqual(len(group['actors']), 3)
        self.assertEqual(len(group['files']), 2)
        self.assertIn(('other', 'unrecovered decoder cause', '-', '-'), groups)
        result = subprocess.run(['python3', str(SCRIPT), '--scripts', str(self.root),
                                 '--rejection-matrix'], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('arity\tQuery\t0\t1\t6\t3\t2\tunknown:', result.stdout)
        self.assertIn('current execution not established', result.stdout)

    def test_checked_in_review_anchors_resolve_without_claiming_tests_ran(self):
        data = json.loads(SCRIPT.with_name('native-behavior.json').read_text())
        root = SCRIPT.resolve().parents[2]
        self.assertTrue(data['natives'])
        self.assertEqual(len({row['native'] for row in data['natives']}), len(data['natives']))
        for row in data['natives'] + data.get('instructions', []):
            for reference in row['tests']:
                path, name = reference.split('::', 1)
                self.assertIn('fn ' + name + '(', (root / path).read_text(), reference)


if __name__ == '__main__':
    unittest.main()
