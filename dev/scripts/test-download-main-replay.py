#!/usr/bin/env python3
"""Verify explicit workflow-run acquisition without a network or game install."""
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest import mock
import zipfile


SPEC = importlib.util.spec_from_file_location(
    "download_main_replay", Path(__file__).with_name("download-main-replay.py"))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)
COMMIT = "a" * 40


class ExactRun(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.destination = self.root / "cyrus"
        self.destination.write_bytes(b"previous")
        package = io.BytesIO()
        with tarfile.open(fileobj=package, mode="w:gz") as archive:
            for name in ["./cyrus", "../unwanted"]:
                member = tarfile.TarInfo(name)
                member.size = len(b"native")
                archive.addfile(member, io.BytesIO(b"native"))
        bundle = io.BytesIO()
        with zipfile.ZipFile(bundle, "w") as archive:
            archive.writestr("cyrus-x86_64-unknown-linux-gnu.tar.gz", package.getvalue())
        self.bundle = bundle.getvalue()
        self.run = dict(id=7, head_sha=COMMIT, path=".github/workflows/pr-artifacts.yml",
                        event="pull_request")
        self.artifact = dict(id=11, name=MODULE.ARTIFACT, expired=False,
                             digest="sha256:" + hashlib.sha256(self.bundle).hexdigest())
        self.calls = []

    def query(self, command, **kwargs):
        self.calls.append(command)
        if command[2].endswith("/artifacts"):
            return json.dumps({"artifacts": [self.artifact]})
        return json.dumps(self.run)

    def transfer(self, command, **kwargs):
        self.calls.append(command)
        self.assertIn("/artifacts/11/zip", command[2])
        kwargs["stdout"].write(self.bundle)
        return subprocess.CompletedProcess(command, 0)

    def acquire(self):
        with mock.patch.object(MODULE.subprocess, "check_output", side_effect=self.query), \
                mock.patch.object(MODULE.subprocess, "run", side_effect=self.transfer):
            return MODULE.download(COMMIT, self.destination, run_id=7)

    def test_fetches_published_pr_artifact_before_other_run_jobs_finish(self):
        receipt = self.acquire()
        self.assertEqual(self.destination.read_bytes(), b"native")
        self.assertTrue(self.destination.stat().st_mode & 0o111)
        self.assertEqual(list(self.root.iterdir()), [self.destination])
        self.assertEqual(receipt["commit"], COMMIT)
        self.assertEqual(receipt["run"], 7)
        self.assertEqual(receipt["artifact"], 11)
        self.assertEqual(receipt["archive_sha256"], self.artifact["digest"].split(":")[1])
        self.assertEqual(receipt["executable_sha256"], hashlib.sha256(b"native").hexdigest())
        self.assertFalse(any(command[1:3] == ["run", "list"] for command in self.calls))

    def test_wrong_head_or_workflow_preserves_previous_binary_without_download(self):
        for field, value in [("head_sha", "b" * 40), ("path", ".github/workflows/ci.yml")]:
            with self.subTest(field=field):
                old = self.run[field]
                self.run[field] = value
                with self.assertRaisesRegex(ValueError, "head|workflow"):
                    self.acquire()
                self.assertEqual(self.destination.read_bytes(), b"previous")
                self.assertFalse(any("/zip" in command[2] for command in self.calls))
                self.run[field] = old

    def test_digest_mismatch_preserves_previous_binary(self):
        self.artifact["digest"] = "sha256:" + "0" * 64
        with self.assertRaisesRegex(ValueError, "digest"):
            self.acquire()
        self.assertEqual(self.destination.read_bytes(), b"previous")
        self.assertEqual(list(self.root.iterdir()), [self.destination])

    def test_missing_or_expired_standard_artifact_is_not_replaced_by_raytracing(self):
        for change in [dict(expired=True), dict(name="native-raytracing-x86_64-unknown-linux-gnu")]:
            original = self.artifact.copy()
            self.artifact.update(change)
            with self.assertRaisesRegex(ValueError, "artifact"):
                self.acquire()
            self.assertEqual(self.destination.read_bytes(), b"previous")
            self.artifact = original

    def test_explicit_run_cannot_be_combined_with_ancestor_fallback(self):
        with self.assertRaisesRegex(ValueError, "fallback"):
            MODULE.download(COMMIT, self.destination, fallback_main=True, run_id=7)
        self.assertEqual(self.destination.read_bytes(), b"previous")


class CheckoutBinding(unittest.TestCase):
    def query(self, command, **kwargs):
        endpoint = command[2]
        if endpoint.endswith('/jobs?per_page=100'):
            return json.dumps({'jobs': [dict(id=12, name='Native (x86_64-unknown-linux-gnu)')]})
        if endpoint.endswith('/logs'):
            return ('2026-11-26T12:00:00Z [command]/usr/bin/git log -1 --format=%H\n'
                    + '2026-11-26T12:00:01Z ' + 'b' * 40 + '\n')
        return json.dumps(dict(sha='b' * 40, tree=dict(sha=self.tree),
                               parents=[dict(sha='e' * 40), dict(sha=COMMIT)]))

    def test_binds_actual_merge_checkout_instead_of_workflow_head(self):
        self.tree = 'c' * 40
        with mock.patch.object(MODULE.subprocess, 'check_output', side_effect=self.query):
            result = MODULE.checkout_source(7, COMMIT, self.tree)
        self.assertEqual(result, dict(checkout_job=12, checkout_commit='b' * 40,
                                      checkout_tree=self.tree))

    def test_wrong_checkout_tree_is_rejected(self):
        self.tree = 'd' * 40
        with mock.patch.object(MODULE.subprocess, 'check_output', side_effect=self.query):
            with self.assertRaisesRegex(ValueError, 'checkout tree'):
                MODULE.checkout_source(7, COMMIT, 'c' * 40)

    def test_missing_log_or_unrelated_head_is_rejected(self):
        self.tree = 'c' * 40
        for missing in (True, False):
            def query(command, **kwargs):
                response = self.query(command, **kwargs)
                if missing and command[2].endswith('/logs'):
                    return 'no checkout record'
                if not missing and '/git/commits/' in command[2]:
                    source = json.loads(response)
                    source['parents'] = []
                    return json.dumps(source)
                return response
            with self.subTest(missing_log=missing):
                with mock.patch.object(MODULE.subprocess, 'check_output', side_effect=query):
                    with self.assertRaisesRegex(ValueError, 'checkout'):
                        MODULE.checkout_source(7, COMMIT, self.tree)

    def test_missing_checkout_evidence_is_not_inferred_from_head(self):
        with mock.patch.object(MODULE.subprocess, 'check_output', return_value='{"jobs": []}'):
            with self.assertRaisesRegex(ValueError, 'native job'):
                MODULE.checkout_source(7, COMMIT, 'c' * 40)


if __name__ == "__main__":
    unittest.main()
