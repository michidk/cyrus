#!/usr/bin/env python3
"""Exercise recovery through the public TCP protocol in separate driver processes."""
import json
import fcntl
from pathlib import Path
import socketserver
import subprocess
import sys
import tempfile
import threading
import unittest

SCRIPT = Path(__file__).with_name('native-session-capture.py')
DOCUMENT = 'CYRUS-SESSION 1\n' + '0' * 64 + '\n' + json.dumps({'version': 1, 'logical': {'registry': {'version': 1, 'worlds': [
    {'world': index, 'content_id': str(index), 'source_identity': str(index),
     'generation': index, 'actors': []} for index in range(2)]}},
    'entity_bindings': [], 'player_model': {}, 'actor_models': {}, 'render_playback': {}})


class Recovery(unittest.TestCase):
    def test_restart_reuses_durable_capture_and_requires_committed_progress(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            binary = output / 'executable'
            binary.write_bytes(b'test executable')
            commands = []
            fresh_runtime = False
            reject = True
            empty = True
            frozen = False
            changed_source = False
            omit_screenshot = False
            clock = 0
            scene = 'START'

            class Handler(socketserver.StreamRequestHandler):
                def handle(self):
                    nonlocal empty, clock, scene, fresh_runtime
                    command = json.loads(self.rfile.readline())['cmd']
                    kind = command['type']
                    commands.append(kind)
                    if empty:
                        empty = False
                        return
                    result = {}
                    ok = True
                    error = 'rejected publication'
                    if kind == 'state':
                        if not frozen:
                            clock += 1
                        result = {'state': 'InGame', 'scene_ready': True, 'scene': scene,
                                  'clock': {'game_seconds': clock}}
                    elif kind == 'load_scene':
                        scene = command['name']
                    elif kind == 'session_capture_result':
                        result = {'pending': False, 'document': DOCUMENT.replace('\"content_id\": \"0\"', '\"content_id\": \"changed\"') if changed_source else DOCUMENT}
                    elif kind == 'session_load':
                        fresh_runtime = False
                    elif kind == 'session_load_result':
                        ok = not reject
                        result = {'pending': False, 'committed': True}
                        if fresh_runtime and command.get('acknowledge'):
                            ok = False
                            error = 'native session load has no terminal result'
                    elif kind == 'screenshot':
                        if not omit_screenshot:
                            Path(command['path']).write_bytes(b'\x89PNG\r\n\x1a\n\x00\x00\x00\x00IEND\xaeB`\x82')
                    self.wfile.write((json.dumps({'ok': ok, 'result': result,
                                                 'error': error}) + '\n').encode())

            with socketserver.TCPServer(('127.0.0.1', 0), Handler) as server:
                worker = threading.Thread(target=server.serve_forever, daemon=True)
                worker.start()
                def run():
                    return subprocess.run([sys.executable, str(SCRIPT), str(server.server_address[1]),
                                           str(output), '--binary', str(binary), '--runtime-output', str(output),
                                           '--seconds', '0.5', '--prepare-seconds', '0.5', '--interval', '0.01'],
                                          capture_output=True, text=True, timeout=4)
                try:
                    first = run()
                    self.assertNotEqual(first.returncode, 0)
                    self.assertTrue((output / 'checkpoint.json').exists())
                    self.assertFalse((output / 'result.json').exists())
                    self.assertEqual(commands.count('load_scene'), 2)
                    commands.clear()
                    reject = False
                    fresh_runtime = True
                    second = run()
                    self.assertEqual(second.returncode, 0, second.stderr)
                    self.assertNotIn('load_scene', commands)
                    self.assertEqual(commands.count('session_capture'), 1)
                    self.assertEqual(json.loads((output / 'result.json').read_text())['publication'], 'committed')
                    frozen = True
                    stalled = run()
                    self.assertNotEqual(stalled.returncode, 0)
                    self.assertFalse((output / 'result.json').exists())
                    frozen = False
                    changed_source = True
                    mismatch = run()
                    self.assertNotEqual(mismatch.returncode, 0)
                    self.assertIn('changed retained world', mismatch.stderr)
                    changed_source = False
                    omit_screenshot = True
                    missing_image = run()
                    self.assertNotEqual(missing_image.returncode, 0)
                    self.assertFalse((output / 'result.json').exists())
                    omit_screenshot = False
                    with (output / 'capture.lock').open('a') as lock:
                        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                        commands.clear()
                        locked = run()
                        self.assertNotEqual(locked.returncode, 0)
                        self.assertFalse(commands)
                    binary.write_bytes(b'changed executable')
                    changed = run()
                    self.assertNotEqual(changed.returncode, 0)
                    self.assertIn('identity mismatch', changed.stderr)
                    binary.write_bytes(b'test executable')
                    (output / 'before.session').write_text(DOCUMENT + ' ')
                    commands.clear()
                    third = run()
                    self.assertNotEqual(third.returncode, 0)
                    self.assertIn('identity mismatch', third.stderr)
                    self.assertFalse(commands)
                    self.assertFalse((output / 'result.json').exists())
                finally:
                    server.shutdown()
                    worker.join()


if __name__ == '__main__':
    unittest.main()
