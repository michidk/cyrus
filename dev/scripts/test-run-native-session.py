#!/usr/bin/env python3
"""Exercise the native runner through a real child process and loopback listener."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('run-native-session.py')
FAKE_GAME = '''#!/usr/bin/env python3
import socket,sys
address=sys.argv[sys.argv.index('--control')+1]
host,port=address.split(':')
with socket.socket() as server:
 server.bind((host,int(port)));server.listen()
 while True:
  connection,_=server.accept()
  with connection:
   if connection.recv(4096):connection.sendall(b'{"ok":true}\\n')
'''


class NativeRuntime(unittest.TestCase):
    def setup_runtime(self, root):
        binary = root / 'game'
        binary.write_text(FAKE_GAME)
        binary.chmod(0o755)
        install = root / 'install'
        install.mkdir()
        (install / 'sentinel').write_text('original')
        cache = root / 'cache'
        cache.mkdir()
        output = root / 'run'
        command = [sys.executable, str(SCRIPT), '--binary', str(binary), '--install', str(install),
                   '--output', str(output), '--cache', str(cache), '--scene', 'fixture',
                   '--lock', str(root / 'live.lock'), '--timeout', '5', '--']
        return command, output, install, cache

    def assert_stopped(self, output):
        receipt = json.loads((output / 'runtime.json').read_text())
        with self.assertRaises(ProcessLookupError):
            os.kill(receipt['pid'], 0)
        return receipt

    def test_public_driver_has_host_config_and_listener_then_child_is_stopped(self):
        with tempfile.TemporaryDirectory() as directory:
            command, output, install, cache = self.setup_runtime(Path(directory))
            driver = '''import os,socket,sys
with socket.create_connection(('127.0.0.1',int(sys.argv[1]))) as stream:
 stream.sendall(b'state');assert stream.recv(100)==b'{"ok":true}\\n'
assert os.environ['CYRUS_CONFIG_DIR']==sys.argv[2]+'/config'
'''
            result = subprocess.run(command + [sys.executable, '-c', driver, '{port}', '{output}'],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            receipt = self.assert_stopped(output)
            self.assertEqual(receipt['phase'], 'completed')
            self.assertEqual((output / 'config/cache').resolve(), cache)
            self.assertEqual((install / 'sentinel').read_text(), 'original')
            self.assertEqual(len(receipt['sha256']), 64)

    def test_driver_failure_retains_diagnostics_and_stops_only_its_child(self):
        with tempfile.TemporaryDirectory() as directory:
            command, output, _, _ = self.setup_runtime(Path(directory))
            result = subprocess.run(command + [sys.executable, '-c', 'print("driver failure evidence", flush=True); raise SystemExit(7)'],
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            receipt = self.assert_stopped(output)
            self.assertEqual(receipt['phase'], 'failed')
            self.assertIn('7', receipt['error'])
            self.assertIn('driver failure evidence', (output / 'process.log').read_text())

    def test_existing_evidence_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            command, output, _, _ = self.setup_runtime(Path(directory))
            output.mkdir()
            (output / 'process.log').write_text('retained evidence')
            result = subprocess.run(command + [sys.executable, '-c', 'pass'], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual((output / 'process.log').read_text(), 'retained evidence')
            self.assertFalse((output / 'runtime.json').exists())

    def test_session_import_response_can_take_longer_than_listener_probe(self):
        with tempfile.TemporaryDirectory() as directory:
            command, output, _, _ = self.setup_runtime(Path(directory))
            binary = Path(directory) / 'game'
            binary.write_text("#!/usr/bin/env python3\nimport json,socket,sys,time\nhost,port=sys.argv[sys.argv.index('--control')+1].split(':')\nwith socket.socket() as server:\n server.bind((host,int(port)));server.listen()\n while True:\n  connection,_=server.accept()\n  with connection,connection.makefile('rw') as stream:\n   for line in stream:\n    request=json.loads(line);kind=request['cmd']['type']\n    if kind=='session_load_result':time.sleep(1.1)\n    result={'state':'InGame','scene_ready':True,'scripts':{'world':1},'examine':{'open':False}} if kind=='state' else {'pending':False,'committed':True}\n    stream.write(json.dumps({'id':request['id'],'ok':True,'result':result})+'\\n');stream.flush()\n")
            session = Path(directory) / 'retained.session'
            session.write_text('public document is consumed by the game')
            command[-1:-1] = ['--session', str(session)]
            result = subprocess.run(command + [sys.executable, '-c', 'pass'],
                                    capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(self.assert_stopped(output)['phase'], 'completed')

    def test_unbounded_timeout_is_rejected_before_launch(self):
        for timeout in ('0', '-1', 'nan', 'inf'):
            with self.subTest(timeout=timeout), tempfile.TemporaryDirectory() as directory:
                command, output, _, _ = self.setup_runtime(Path(directory))
                command[command.index('--timeout') + 1] = timeout
                result = subprocess.run(command + [sys.executable, '-c', 'pass'],
                                        capture_output=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse(output.exists())

    def test_install_cannot_be_used_as_writable_cache(self):
        with tempfile.TemporaryDirectory() as directory:
            command, output, install, _ = self.setup_runtime(Path(directory))
            command[command.index('--cache') + 1] = str(install)
            result = subprocess.run(command + [sys.executable, '-c', 'pass'], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((output / 'config/cache').exists())

    def test_install_cannot_be_used_as_output(self):
        with tempfile.TemporaryDirectory() as directory:
            command, _, install, _ = self.setup_runtime(Path(directory))
            command[command.index('--output') + 1] = str(install / 'run')
            result = subprocess.run(command + [sys.executable, '-c', 'pass'], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((install / 'run').exists())


if __name__ == '__main__':
    unittest.main()
