#!/usr/bin/env python3
"""Drive continuation and restart through the real public TCP client."""
import json
from pathlib import Path
import runpy
import socketserver
import subprocess
import sys
import tempfile
import threading
import unittest

HELPERS = runpy.run_path(str(Path(__file__).with_name('test-native-session-assertions.py')))
SCRIPT = Path(__file__).with_name('native-session-capture.py')


class Continuation(unittest.TestCase):
    def test_armed_first_update_and_durable_resume(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            binary = output / 'executable'
            binary.write_bytes(b'fixture binary')
            sample = HELPERS['snapshot']()
            registry = sample['logical']['registry']
            registry.update(version=1, current=3)
            registry['worlds'].append(json.loads(json.dumps(registry['worlds'][0])))
            registry['worlds'][1]['world'] = 4
            for world in registry['worlds']:
                world.update(content_id=str(world['world']), source_identity=str(world['world']))
            sample.update(version=1, player_model={}, actor_models={}, entity_bindings=[], render_playback={})
            document = 'CYRUS-SESSION 1\n' + '0' * 64 + '\n' + json.dumps(sample)
            seed = output / 'seed.session'
            seed.write_text(document)
            condition = threading.Condition()
            generation, current, clock = 1, 3, 0
            pending, terminal = False, False
            commands = []

            class Handler(socketserver.StreamRequestHandler):
                def handle(self):
                    nonlocal generation, current, clock, pending, terminal
                    cmd = json.loads(self.rfile.readline())['cmd']
                    kind = cmd['type']
                    commands.append(kind)
                    result, ok, error = {}, True, ''
                    if kind == 'state':
                        clock += 0.5
                        result = {'state': 'InGame', 'scene': 'ACTIVE' if current == 3 else 'DORMANT',
                                  'scene_ready': True, 'clock': {'game_seconds': clock},
                                  'rendered_actor_pending': pending}
                    elif kind == 'scripts':
                        result = {'generation': generation}
                    elif kind == 'rendered_actor':
                        if 'after_generation' in cmd:
                            with condition:
                                pending = True
                                condition.wait_for(lambda: generation != cmd['after_generation'], timeout=3)
                                pending = False
                        result = HELPERS['observation']()
                        result['rendered_actor'].update(world=current, owner=100 + generation)
                        result['rendered_actor']['actor']['generation'] = generation
                        result['rendered_actor']['attachments'][0]['owner'] = 100 + generation
                    elif kind in ('session_load', 'load_scene'):
                        with condition:
                            generation += 1
                            current = 4 if kind == 'load_scene' else 3
                            clock = 0
                            terminal = True
                            condition.notify_all()
                    elif kind == 'session_load_result':
                        if not terminal:
                            ok, error = False, 'native session load has no terminal result'
                        result = {'pending': False, 'committed': True}
                        if cmd.get('acknowledge'):
                            terminal = False
                    elif kind == 'session_capture_result':
                        result = {'pending': False, 'document': document}
                    elif kind == 'screenshot':
                        Path(cmd['path']).write_bytes(b'\x89PNG\r\n\x1a\n\x00\x00\x00\x00IEND\xaeB`\x82')
                    self.wfile.write((json.dumps({'ok': ok, 'result': result, 'error': error}) + '\n').encode())

            with socketserver.ThreadingTCPServer(('127.0.0.1', 0), Handler) as server:
                server.daemon_threads = True
                worker = threading.Thread(target=server.serve_forever, daemon=True)
                worker.start()
                try:
                    def run():
                        return subprocess.run([sys.executable, str(SCRIPT), str(server.server_address[1]),
                            str(output), '--binary', str(binary), '--runtime-output', str(output),
                            '--continuation-seed', str(seed), '--runtime-seed', str(seed),
                            '--attachment-actor', '0', '--revisit', 'DORMANT', '--revisit-actor', '0',
                            '--seconds', '2', '--prepare-seconds', '2', '--interval', '0.01'],
                            capture_output=True, text=True, timeout=10)
                    first = run()
                    self.assertEqual(first.returncode, 0, first.stderr)
                    self.assertEqual(commands.count('session_load'), 2)
                    self.assertEqual(commands.count('session_capture'), 2)
                    self.assertTrue(json.loads((output / 'result.json').read_text())['dormant_revisit_attachment'])
                    commands.clear()
                    resumed = run()
                    self.assertEqual(resumed.returncode, 0, resumed.stderr)
                    self.assertEqual(commands.count('session_load'), 1)
                    self.assertEqual(commands.count('session_capture'), 1)
                finally:
                    server.shutdown()
                    worker.join()


if __name__ == '__main__':
    unittest.main()
