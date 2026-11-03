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

    def test_operand_reviews_do_not_infer_completion_from_portable_scope_presence(self):
        self.path.write_text(json.dumps({'operands': [dict(
            self.row, operand='value.integer')]}))
        result = subprocess.run(['python3', str(SCRIPT), '--operand-matrix',
                                 '--behavior', str(self.path)], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = {line.split('\t')[0]: line.split('\t')
                for line in result.stdout.splitlines()[1:]}
        self.assertEqual(len(rows), 15)
        self.assertEqual(rows['value.integer'][1:3], ['partial', '#5'])
        self.assertEqual(rows['place.property'][1:3], ['unaudited', '-'])
        for row in [dict(self.row, operand='invented'),
                    dict(self.row, operand='value.integer', status='done')]:
            self.path.write_text(json.dumps({'operands': [row]}))
            result = subprocess.run(['python3', str(SCRIPT), '--operand-matrix',
                                     '--behavior', str(self.path)], capture_output=True, text=True)
            self.assertEqual(result.returncode, 1)
            self.assertIn('invalid operand audit', result.stderr)

    def test_local_targets_exclude_plain_locals_and_only_mark_same_actor_slot_candidates(self):
        def write(index):
            return {'operation': {'type': 'assign', 'destination': {'type': 'local', 'index': index},
                                  'formula': {'first': {'type': 'integer', 'value': 7}}}}
        data = {'actors': {
            'CONSUMER': {'program': {'steps': {
                '0': write(1), '1': write(2),
                '2': {'operation': {'type': 'call', 'call': {
                    'target': {'type': 'local', 'index': 1}, 'arguments': []}}},
                '3': {'operation': {'type': 'assign', 'destination': {'type': 'flag', 'index': 0},
                      'formula': {'first': {'type': 'read', 'place': {'type': 'property',
                                  'object': {'type': 'local', 'index': 1}, 'reference': 0}}}}},
            }}},
            'OTHER': {'program': {'steps': {'0': write(1)}}},
        }}
        (self.root / 'world.scripts').write_text(json.dumps(data))
        result = subprocess.run(['python3', str(SCRIPT), '--scripts', str(self.root),
                                 '--local-targets'], capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        report = json.loads(result.stdout)
        self.assertEqual(report['evidence'], 'STATIC_ONLY')
        self.assertEqual(len(report['consumers']), 2)
        self.assertEqual(report['same_slot_write_candidates'], [{
            'file': 'world.scripts', 'actor': 'CONSUMER', 'step': '0', 'index': 1,
            'operation': 'assign'}])
        self.assertEqual(report['local_writes']['assign'], 3)
        self.assertIn('not reaching definitions', report['limits'])
        with self.assertRaises(ValueError):
            audit.local_target_census(self.root / 'absent', '*.scripts')

    def test_property_modes_are_static_and_updates_are_not_double_counted(self):
        prop = {'type': 'property', 'object': {'type': 'me'}, 'reference': 0}
        data = {'definition': {'references': ['heading', 'other']}, 'actors': {
            'A': {'program': {'steps': {
                '0': {'operation': {'type': 'assign', 'destination': prop,
                      'formula': {'first': {'type': 'read', 'place': prop}}}},
                '1': {'operation': {'type': 'increment', 'destination': prop, 'delta': 1}},
                '2': {'operation': {'type': 'if', 'conditions': [
                      {'lhs': {'type': 'update', 'place': prop, 'delta': -1}}]}},
            }}}}}
        (self.root / 'one.scripts').write_text(json.dumps(data))
        self.path.write_text(json.dumps({'properties': [dict(self.row, property='heading')]}))
        result = subprocess.run(['python3', str(SCRIPT), '--scripts', str(self.root),
                                 '--property-matrix', '--behavior', str(self.path)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        rows = {line.split('\t')[0]: line.split('\t')
                for line in result.stdout.splitlines()[1:]}
        self.assertEqual(rows['heading'][1:6], ['1', '1', '2', 'partial', '#5'])
        self.assertEqual(rows['other'][1:5], ['0', '0', '0', 'unaudited'])
        prop['reference'] = 99
        (self.root / 'one.scripts').write_text(json.dumps(data))
        result = subprocess.run(['python3', str(SCRIPT), '--scripts', str(self.root),
                                 '--property-matrix', '--behavior', str(self.path)],
                                capture_output=True, text=True)
        self.assertEqual(result.returncode, 1)
        self.assertIn('property reference 99 absent', result.stderr)

    def test_property_groups_expand_only_explicit_members(self):
        group = dict(self.row, properties=['Reviewed'], status='missing')
        self.path.write_text(json.dumps({'property_groups': [group]}))
        rows = dict(audit.behavior_rows(self.path, self.catalogue, self.used,
                                        'properties', 'property'))
        self.assertEqual(rows['Reviewed']['status'], 'missing')
        self.assertEqual(rows['Reviewed']['property'], 'Reviewed')
        self.assertIsNone(rows['Mentioned'])

    def test_property_groups_reject_ambiguous_or_incomplete_reviews(self):
        group = dict(self.row, properties=['Reviewed'], status='missing')
        variants = [
            {'property_groups': [dict(group, properties=[])]},
            {'property_groups': [dict(group, properties='Reviewed')]},
            {'property_groups': [dict(group, properties=[None])]},
            {'property_groups': [dict(group, properties=[''])]},
            {'property_groups': [dict(group, properties=['Unknown'])]},
            {'property_groups': [dict(group, properties=['Reviewed', 'Reviewed'])]},
            {'property_groups': [group, group]},
            {'properties': [dict(self.row, property='Reviewed')],
             'property_groups': [group]},
            {'property_groups': [dict(group, status='done')]},
        ]
        for data in variants:
            with self.subTest(data=data):
                self.path.write_text(json.dumps(data))
                with self.assertRaises(ValueError):
                    audit.behavior_rows(self.path, self.catalogue, self.used,
                                        'properties', 'property')

    def test_checked_in_review_anchors_resolve_without_claiming_tests_ran(self):
        data = json.loads(SCRIPT.with_name('native-behavior.json').read_text())
        root = SCRIPT.resolve().parents[2]
        self.assertTrue(data['natives'])
        self.assertEqual(len({row['native'] for row in data['natives']}), len(data['natives']))
        for row in data['natives'] + data.get('instructions', []) + data.get('operands', []) + data.get('properties', []) + data.get('property_groups', []):
            for reference in row['tests']:
                path, name = reference.split('::', 1)
                self.assertIn('fn ' + name + '(', (root / path).read_text(), reference)


if __name__ == '__main__':
    unittest.main()
