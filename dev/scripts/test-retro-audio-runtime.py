#!/usr/bin/env python3
"""Check reference runtime acquisition, reset recovery and source invalidation."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
DOCKER = '''#!/usr/bin/env python3
import hashlib,io,json,os,sys,tarfile
from pathlib import Path
p=Path(os.environ['FAKE_DOCKER_STATE']);s=json.loads(p.read_text());a=sys.argv[1:]
s['calls'].append(a)
code=0
if a[0]=='info':pass
elif a[:2]==['image','inspect']:
    image=a[-1];code=0 if image in s['images'] else 1
    if not code and '--format' in a:print('sha256:'+hashlib.sha256(image.encode()).hexdigest())
elif a[0]=='build':s['images'].append(a[a.index('-t')+1])
elif a[0]=='save':
    image=a[-1];data=image.encode();config=hashlib.sha256(data).hexdigest()+'.json'
    with tarfile.open(a[a.index('--output')+1],'w') as archive:
        for name,payload in [(config,data),('manifest.json',json.dumps([{'Config':config,'RepoTags':[image],'Layers':[]}]).encode())]:
            entry=tarfile.TarInfo(name);entry.size=len(payload);archive.addfile(entry,io.BytesIO(payload))
elif a[0]=='load':
    with tarfile.open(a[a.index('--input')+1]) as archive:
        s['images'].extend(json.load(archive.extractfile('manifest.json'))[0]['RepoTags'])
elif a[0]=='run':
    if '--rm' in a and '/bin/true' in a:pass
    else:print(os.environ.get('FAKE_MARKER','single-cd-volume-v1'))
else:raise RuntimeError(a)
p.write_text(json.dumps(s));sys.exit(code)
'''


class Runtime(unittest.TestCase):
    def test_cold_build_reset_restore_changed_inputs_and_invalid_marker(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            for relative in ['dev/scripts/ensure-retro-audio-runtime.sh', 'dev/scripts/ensure-runtime-image.sh',
                             'dev/docker/retro/Dockerfile.audio', 'dev/docker/retro/Dockerfile.audio-build', 'dev/docker/retro/cd-volume.patch',
                             'dev/docker/retro/entrypoint.sh', 'dev/docker/retro/dosbox-x.conf']:
                target = root / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(ROOT / relative, target)
            (root / 'magic/build-tools').mkdir(parents=True)
            binary = root / 'bin/docker'
            binary.parent.mkdir()
            binary.write_text(DOCKER)
            binary.chmod(0o755)
            # Fetch/extract/compile are external processes in this orchestration test.
            # The independent compiled CD-control test checks the source patch.
            stubs = {
                'curl': '#!/bin/sh\nprintf source > "$4"\n',
                'tar': '#!/bin/sh\nexit 0\n',
                'patch': '#!/bin/sh\ncat >/dev/null\n',
                'sha256sum': '#!/bin/sh\nif [ "${1:-}" = --check ]; then cat >/dev/null; exit 0; fi\nexec /usr/bin/sha256sum "$@"\n',
            }
            for name, text in stubs.items():
                path = binary.parent / name
                path.write_text(text)
                path.chmod(0o755)
            state = root / 'docker.json'
            state.write_text(json.dumps({'images': [], 'calls': []}))
            env = dict(os.environ, PATH=f'{binary.parent}:{os.environ["PATH"]}', FAKE_DOCKER_STATE=str(state))
            command = [str(root / 'dev/scripts/ensure-retro-audio-runtime.sh')]
            first = subprocess.check_output(command, env=env, text=True).strip()
            self.assertTrue(first.startswith('cyrus-retro-audio:'))
            saved = json.loads(state.read_text())
            self.assertEqual(sum(c[0] == 'build' for c in saved['calls']), 2)
            helper = root / 'dev/scripts/ensure-retro-audio-runtime.sh'
            helper.write_text(helper.read_text() + '\n# Orchestration-only edits retain unchanged image inputs.\n')
            self.assertEqual(subprocess.check_output(command, env=env, text=True).strip(), first)
            saved = json.loads(state.read_text())
            saved['images'] = []
            state.write_text(json.dumps(saved))
            restored = subprocess.check_output(command, env=env, text=True).strip()
            self.assertEqual(restored, first)
            saved = json.loads(state.read_text())
            self.assertEqual(sum(c[0] == 'build' for c in saved['calls']), 2)
            self.assertEqual(sum(c[0] == 'load' for c in saved['calls']), 1)
            patch = root / 'dev/docker/retro/cd-volume.patch'
            patch.write_text(patch.read_text() + '\n')
            changed = subprocess.check_output(command, env=env, text=True).strip()
            self.assertNotEqual(changed, first)
            saved = json.loads(state.read_text())
            self.assertEqual(sum(c[0] == 'build' for c in saved['calls']), 3)
            result = subprocess.run(command, env=dict(env, FAKE_MARKER='unpatched'), capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(result.stdout, '')
            self.assertIn('no single-application CD volume contract', result.stderr)


if __name__ == '__main__':
    unittest.main()
