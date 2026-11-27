#!/usr/bin/env python3
"""The package source record names the actual checkout, not the PR head."""
import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import artifact_provenance as provenance


class ProducerTests(unittest.TestCase):
    def test_records_checkout_tree_attempt_and_exact_package_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            package = folder / 'cyrus-x86_64-unknown-linux-gnu.tar.gz'
            package.write_bytes(b'packaged executable')
            environment = dict(GITHUB_REPOSITORY='michidk/cyrus',
                               GITHUB_RUN_ID='7',GITHUB_RUN_ATTEMPT='2')
            with mock.patch.object(provenance.subprocess,'run') as clean, \
                    mock.patch.object(provenance.subprocess,'check_output',side_effect=['b'*40,'c'*40]):
                record=provenance.write_provenance(folder,'a'*40,environment)
            clean.assert_called_once_with(['git','diff','--quiet','HEAD'],check=True)
            self.assertEqual(record['checkout_commit'],'b'*40)
            self.assertEqual(record['checkout_tree'],'c'*40)
            self.assertEqual(record['head'],'a'*40)
            self.assertEqual(record['attempt'],2)
            self.assertEqual(record['packages'],{package.name:hashlib.sha256(package.read_bytes()).hexdigest()})
            self.assertEqual(json.loads((folder/'source-provenance.json').read_text()),record)

    def test_empty_package_directory_cannot_publish_provenance(self):
        with tempfile.TemporaryDirectory() as temporary, mock.patch.object(provenance.subprocess,'run'):
            with self.assertRaisesRegex(ValueError,'packaged'):
                provenance.write_provenance(Path(temporary),'a'*40,{})
            self.assertFalse((Path(temporary)/'source-provenance.json').exists())

    def test_standard_upload_contains_provenance_before_optional_build(self):
        workflow=Path('.github/workflows/pr-artifacts.yml').read_text()
        writer=workflow.index('Record standard package source provenance')
        upload=workflow.index('name: native-${{ matrix.target }}')
        optional=workflow.index('Build and package ray-tracing game')
        self.assertLess(writer,upload)
        self.assertLess(upload,optional)
        self.assertIn('target/artifacts/source-provenance.json',workflow[upload:optional])


if __name__=='__main__': unittest.main()
