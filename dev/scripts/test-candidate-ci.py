#!/usr/bin/env python3
"""Exercise immutable candidate admission and the shared CI job contract."""
import os
from pathlib import Path
import re
import subprocess
import tempfile
import textwrap
import unittest


WORKFLOW = Path(__file__).parents[2] / '.github/workflows/ci.yml'


class CandidateTests(unittest.TestCase):
    def admission(self, event, candidate, event_commit='a' * 40):
        workflow = WORKFLOW.read_text()
        source = workflow.split('  source:\n', 1)[1].split('\n  packaging-macos:', 1)[0]
        script = textwrap.dedent(source.split('        run: |\n', 1)[1])
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / 'outputs'
            environment = dict(os.environ, EVENT_NAME=event, CANDIDATE_COMMIT=candidate,
                               EVENT_COMMIT=event_commit, GITHUB_OUTPUT=str(output))
            result = subprocess.run(['bash', '-euo', 'pipefail', '-c', script],
                                    env=environment, capture_output=True, text=True)
            return result, output.read_text() if output.exists() else ''

    def test_manual_source_is_required_and_never_interpreted_as_shell(self):
        for value in ('', 'main', 'a' * 39, 'a' * 41, 'g' * 40, '$(exit 42)', 'a' * 40 + '\nmain'):
            with self.subTest(value=value):
                result, output = self.admission('workflow_dispatch', value)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(output, '')

    def test_manual_commit_and_automatic_merge_commit_are_retained_exactly(self):
        for event, candidate, expected in [('workflow_dispatch', 'b' * 40, 'b' * 40),
                                            ('push', '', 'a' * 40),
                                            ('pull_request', 'b' * 40, 'a' * 40)]:
            result, output = self.admission(event, candidate)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(output, 'commit=' + expected + '\n')

    def test_all_existing_gates_use_admitted_source_without_checkout_credentials(self):
        workflow = WORKFLOW.read_text()
        self.assertIn('  workflow_dispatch:', workflow)
        self.assertIn('  contents: read', workflow)
        for name in ('packaging-macos', 'test', 'lints', 'web'):
            job = re.split(r'\n  [a-z-]+:\n', workflow.split('  ' + name + ':\n', 1)[1])[0]
            self.assertIn('needs: source', job)
            self.assertIn('ref: ${{ needs.source.outputs.commit }}', job)
            self.assertIn('persist-credentials: false', job)
            self.assertIn('test "$(git rev-parse HEAD)" = "$EXPECTED_COMMIT"', job)
        for command in ('dev/scripts/gates.sh test', 'dev/scripts/gates.sh lints',
                        'cargo check -p cyrus-web -p cyrus-viewer -p cyrus-assets-worker --target wasm32-unknown-unknown'):
            self.assertIn(command, workflow)
        self.assertIn('--file original-runtime-test', workflow)
        self.assertIn('cargo test --locked --workspace --all-features --no-run --message-format=json', workflow)
        self.assertIn('collect-test-executable.py --bundle collect', workflow)
        self.assertIn('--file manifest.json', workflow)
        self.assertIn('name: workspace-tests-linux', workflow)
        self.assertIn('artifact_provenance.py', workflow)
        self.assertNotIn('secrets.', workflow)


if __name__ == '__main__':
    unittest.main()
