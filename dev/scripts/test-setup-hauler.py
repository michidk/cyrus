#!/usr/bin/env python3
"""Exercise setup against an already restored pinned launcher without npm writes."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('setup-hauler.sh').resolve()


class Setup(unittest.TestCase):
    def test_restored_pinned_launcher_does_not_require_global_reinstallation(self):
        self.check_setup()

    def test_stale_marketplace_is_rebound_to_the_restored_package(self):
        self.check_setup(stale_marketplace=True)

    def check_setup(self, stale_marketplace=False):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            scripts = root / 'dev/scripts'
            scripts.mkdir(parents=True)
            shutil.copyfile(SCRIPT, scripts / SCRIPT.name)
            tools = root / 'bin'
            tools.mkdir()
            installed = root / 'installed/cargo-hauler/dist/bin'
            installed.mkdir(parents=True)
            for path, body in {
                installed / 'hauler': '#!/bin/sh\necho cargo-hauler 0.14.1\n',
                tools / 'node': '#!/bin/sh\nexit 0\n',
                tools / 'npm': '#!/bin/sh\necho "unexpected npm $*" >&2\nexit 77\n',
            }.items():
                path.write_text(body)
                path.chmod(0o755)
            (tools / 'hauler').symlink_to(installed / 'hauler')
            if stale_marketplace:
                claude = tools / 'claude'
                claude.write_text("#!/usr/bin/python3\n"
                                  "import json,sys\nfrom pathlib import Path\n"
                                  "args=sys.argv[1:]\n"
                                  "if args[:3] == ['plugin','marketplace','list']:\n"
                                  " print(json.dumps([dict(name='cargo-hauler-marketplace',source='directory',path='/retired/package')]))\n"
                                  "elif args[:3] == ['plugin','marketplace','remove']:\n"
                                  " Path('removed').touch()\n"
                                  "elif args[:3] == ['plugin','marketplace','add']:\n"
                                  " assert Path('removed').exists();Path('rebound').touch()\n"
                                  "elif args[:2] == ['plugin','install']:\n"
                                  " sys.exit(0 if Path('rebound').exists() else 78)\n"
                                  "else: sys.exit(79)\n")
                claude.chmod(0o755)
            result = subprocess.run(['bash', str(scripts / SCRIPT.name)], cwd=root,
                                    env={**os.environ, 'PATH': str(tools) + ':/usr/bin:/bin'},
                                    capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn('cargo-hauler 0.14.1 ready', result.stdout)


if __name__ == '__main__':
    unittest.main()
