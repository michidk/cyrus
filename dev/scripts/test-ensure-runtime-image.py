#!/usr/bin/env python3
"""Public recovery contract without Docker builds or shared runtime changes."""
import os
import hashlib
import io
import json
import tarfile
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('ensure-runtime-image.sh')

def image_archive(path, payload, oci=False, index=False):
    digest = hashlib.sha256(payload).hexdigest()
    config = f'{digest}.json'
    manifest = json.dumps([{'Config': config, 'RepoTags': ['fixture:1'], 'Layers': []}]).encode()
    identity = f'sha256:{digest}'
    entries = [(config, payload), ('manifest.json', manifest)]
    if oci:
        document = json.dumps({'config': {'digest': identity}}).encode()
        digest = hashlib.sha256(document).hexdigest()
        entries.append((f'blobs/sha256/{digest}', document))
        identity = f'sha256:{digest}'
        if index:
            document = json.dumps({'manifests': [{'digest': identity}]}).encode()
            digest = hashlib.sha256(document).hexdigest()
            entries.append((f'blobs/sha256/{digest}', document))
            identity = f'sha256:{digest}'
    with tarfile.open(path, 'w') as archive:
        for name, data in entries:
            entry = tarfile.TarInfo(name)
            entry.size = len(data)
            archive.addfile(entry, io.BytesIO(data))
    return identity


class Recovery(unittest.TestCase):
    def run_restore(self, present=False, archive=True, accepted=True, correct_tag=True, prepared=True, interrupted=False):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            image = root / 'present'
            if present:
                image.touch()
            saved = root / 'runtime.tar'
            if archive:
                saved.write_bytes(b'fixture')
            docker = root / 'docker'
            docker.write_text('#!/bin/sh\necho "$*" >> "$CALLS"\n'
                'if [ "$1" = image ]; then test -f "$PRESENT"; '
                'elif [ "$1" = info ]; then echo fixture-daemon; '
                'elif [ "$1" = run ]; then test "$PREPARED" = yes; '
                'elif [ "$1" = load ]; then test "$ACCEPTED" = yes || exit 1; [ "$CORRECT_TAG" = no ] || touch "$PRESENT"; test "$INTERRUPTED" = no; fi\n')
            docker.chmod(0o755)
            result = subprocess.run(['bash', str(SCRIPT), 'fixture:1', str(saved)],
                env=dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}', PRESENT=str(image),
                         CALLS=str(root/'calls'), ACCEPTED='yes' if accepted else 'no', CORRECT_TAG='yes' if correct_tag else 'no', PREPARED='yes' if prepared else 'no', INTERRUPTED='yes' if interrupted else 'no'),
                text=True, capture_output=True)
            state = root/'runtime.tar.restore-state.json'
            log = root/'runtime.tar.restore.log'
            result.restore_state = json.loads(state.read_text()) if state.exists() else None
            result.restore_log = log.read_text() if log.exists() else ''
            return result, (root/'calls').read_text()

    def test_daemon_replacement_restores_only_missing_image(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            saved = root / 'runtime.tar'
            saved.write_bytes(b'fixture')
            docker = root / 'docker'
            docker.write_text('#!/bin/sh\necho "$*" >> "$CALLS"\n'
                'case "$1" in\n'
                'info) echo "$DAEMON";;\n'
                'image) test -f "$PRESENT";;\n'
                'load) touch "$PRESENT";;\n'
                'run) exit 0;;\nesac\n')
            docker.chmod(0o755)
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}',
                       PRESENT=str(root / 'present'), CALLS=str(root / 'calls'), DAEMON='first')
            command = ['bash', str(SCRIPT), 'fixture:1', str(saved)]
            subprocess.run(command, env=env, check=True, capture_output=True)
            (root / 'present').unlink()
            env['DAEMON'] = 'replacement'
            resumed = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertEqual(resumed.returncode, 0, resumed.stderr)
            self.assertIn('runtime daemon changed', resumed.stderr)
            subprocess.run(command, env=env, check=True, capture_output=True)
            self.assertEqual((root / 'calls').read_text().count('load --input'), 2)
            state = json.loads(Path(str(saved) + '.restore-state.json').read_text())
            self.assertEqual((state['daemon'], state['status']), ('replacement', 'succeeded'))

    def test_failed_load_with_retained_metadata_resumes_preparation(self):
        result, calls = self.run_restore(interrupted=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls.count('load --input'), 1)
        self.assertIn('run --rm', calls)
        self.assertEqual(result.restore_state['stage'], 'complete')
        self.assertIn('fixture-daemon', result.restore_log)
        self.assertIn('"stage": "load", "status": "failed"', result.restore_log)

    def test_failed_unpack_has_durable_stage_and_daemon_diagnostics(self):
        result, calls = self.run_restore(present=True, prepared=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.restore_state['stage'], 'prepare')
        self.assertEqual(result.restore_state['status'], 'failed')
        self.assertNotEqual(result.restore_state['exit_code'], 0)
        self.assertIn('fixture-daemon', result.restore_log)
        self.assertIn('run --rm', result.restore_log)
        self.assertNotIn('load --input', calls)

    def test_retain_available_image_atomically_and_reuse_after_reset(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            present = root / 'present'
            template = root / 'template.tar'
            first_id = image_archive(template, b'first-image')
            present.write_text(first_id)
            saved = root / 'nested/runtime.tar'
            docker = root / 'docker'
            docker.write_text("""#!/bin/sh
echo "$*" >> "$CALLS"
case "$1" in
 image) test -f "$PRESENT" && cat "$PRESENT" ;;
 save) cp "$TEMPLATE" "$3"; test ! -f "$FAIL" ;;
 load) test -s "$3" && cp "$IDENTITY" "$PRESENT" ;;
esac
""")
            docker.chmod(0o755)
            env = dict(os.environ, PATH=f'{root}:{os.environ["PATH"]}',
                       PRESENT=str(present), CALLS=str(root/'calls'), FAIL=str(root/'fail'),
                       TEMPLATE=str(template), IDENTITY=str(root/'identity'))
            command = ['bash', str(SCRIPT), '--retain', 'fixture:1', str(saved)]
            (root/'fail').touch()
            failed = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertNotEqual(failed.returncode, 0)
            self.assertFalse(saved.exists(), 'failed save published a reusable archive')
            self.assertFalse(list(saved.parent.glob('*.partial.*')))
            (root/'fail').unlink()
            # Concurrent admissions share one archive; only one successful save runs.
            attempts = [subprocess.Popen(command, env=env, stdout=subprocess.PIPE,
                                         stderr=subprocess.PIPE, text=True) for _ in range(2)]
            for attempt in attempts:
                output, error = attempt.communicate(timeout=10)
                self.assertEqual(attempt.returncode, 0, output + error)
            self.assertEqual(saved.read_bytes(), template.read_bytes())
            subprocess.run(command, env=env, check=True, capture_output=True)
            self.assertEqual((root/'calls').read_text().count('save --output'), 2)
            # Rebuilding the same tag must replace its old checkpoint.
            second_id = image_archive(template, b'rebuilt-image', oci=True)
            present.write_text(second_id)
            subprocess.run(command, env=env, check=True, capture_output=True)
            self.assertEqual(saved.read_bytes(), template.read_bytes(),
                             'retention kept an obsolete runtime under the rebuilt tag')
            self.assertNotEqual(first_id, second_id)
            previous = saved.read_bytes()
            template.write_bytes(b'malformed Docker save')
            present.write_text('sha256:changed')
            invalid = subprocess.run(command, env=env, capture_output=True, text=True)
            self.assertNotEqual(invalid.returncode, 0)
            self.assertEqual(saved.read_bytes(), previous)
            self.assertFalse(list(saved.parent.glob('*.partial.*')))
            # Multi-platform stores may identify an image by its enclosing index.
            second_id = image_archive(template, b'rebuilt-image', oci=True, index=True)
            present.write_text(second_id)
            subprocess.run(command, env=env, check=True, capture_output=True)
            self.assertEqual(saved.read_bytes(), template.read_bytes())
            (root/'identity').write_text(second_id)
            present.unlink()
            subprocess.run(command, env=env, check=True, capture_output=True)
            self.assertTrue(present.exists())
            self.assertEqual((root/'calls').read_text().count('load --input'), 1)

    def test_metadata_after_interrupted_unpack_requires_container_preparation(self):
        result, calls = self.run_restore(present=True, archive=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('run --rm --pull never --network none --read-only --entrypoint /bin/true fixture:1', calls)
        self.assertNotIn('load --input', calls, 'stored layers must be reused after interrupted unpack')

    def test_unpack_failure_is_rejected_without_deleting_or_reloading_metadata(self):
        result, calls = self.run_restore(present=True, prepared=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('run --rm', calls)
        self.assertNotIn('load --input', calls)
        self.assertNotIn('image rm', calls)

    def test_existing_image_needs_no_archive_or_load(self):
        result, calls = self.run_restore(present=True, archive=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn('load', calls)

    def test_erased_image_restores_and_verifies_requested_identity(self):
        result, calls = self.run_restore()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls.count('load --input'), 1)
        self.assertEqual(result.restore_state['stage'], 'complete')

    def test_successful_load_with_wrong_tag_is_rejected(self):
        result, calls = self.run_restore(correct_tag=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.restore_state['status'], 'failed')
        self.assertEqual(result.restore_state['exit_code'], 1)
        self.assertIn('did not restore requested image fixture:1', result.stderr)
        self.assertIn('load --input', calls)

    def test_missing_archive_or_failed_load_does_not_start_runtime(self):
        for options in [dict(archive=False), dict(accepted=False)]:
            result, _ = self.run_restore(**options)
            self.assertNotEqual(result.returncode, 0)

if __name__ == '__main__':
    unittest.main()
