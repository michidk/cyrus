#!/usr/bin/env python3
"""Drive the public build script through interrupted compilation and packaging."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parent


class Recovery(unittest.TestCase):
    def test_completed_packages_survive_target_loss_and_packaging_failure(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            scripts = root / 'dev/scripts'
            scripts.mkdir(parents=True)
            for name in ('build-web.sh', 'web-build-checkpoint.py', 'build-provenance.py', 'resolve-wasm-bindgen.py', 'web-compile-checkpoint.py'):
                shutil.copyfile(SCRIPTS / name, scripts / name)
            (scripts / 'toolchain.sh').write_text('tools="$PWD/tools"; export PATH="$PWD/bin:$PATH"\n')
            (scripts / 'cargo.sh').write_text('cargo() { "$PWD/bin/fake-cargo" "$@"; }\n')
            (root / 'bin').mkdir()
            commands = {
                'rustc': '#!/bin/sh\necho compiler-version\n',
                'fake-cargo': '''#!/bin/bash
set -eu
package="$3"
echo "$package" >> "$PWD/cargo.log"
if [ "$package" = cyrus-web ] && [ -e "$PWD/fail-compile" ]; then exit 7; fi
mkdir -p "$CARGO_TARGET_DIR/wasm32-unknown-unknown/wasm-dev"
printf 'named wasm %s' "$package" > "$CARGO_TARGET_DIR/wasm32-unknown-unknown/wasm-dev/${package//-/_}.wasm"
if [ "$package" = cyrus-web ]; then mv "$CARGO_TARGET_DIR/wasm32-unknown-unknown/wasm-dev/cyrus_web.wasm" "$CARGO_TARGET_DIR/wasm32-unknown-unknown/wasm-dev/cyrus-web.wasm"; fi
''',
                'wasm-bindgen': '''#!/bin/bash
set -eu
if [ "$1" = --version ]; then echo 'wasm-bindgen 0.2.130'; exit; fi
if [ -e "$PWD/fail-package" ]; then exit 8; fi
[ "$1" = --keep-debug ] || exit 9
while [ "$#" -gt 1 ]; do
case "$1" in --out-dir) out="$2"; shift;; --out-name) name="$2"; shift;; esac
shift
done
mkdir -p "$out"
cp "$1" "$out/${name}_bg.wasm"
printf 'javascript' > "$out/$name.js"
''',
            }
            for name, code in commands.items():
                path = root / 'bin' / name
                path.write_text(code)
                path.chmod(0o755)
            for name in ('cyrus-web',):
                page = root / 'crates' / name / 'web/index.html'
                page.parent.mkdir(parents=True)
                page.write_text('page')
            for name in ('asset-worker.js', 'game-assets.js', 'loader.js', 'install.js', 'local-install.js', 'sw.js', 'loading.css'):
                path = root / 'dev/web' / name
                path.parent.mkdir(exist_ok=True)
                path.write_text(name)
            (root / 'Cargo.lock').write_text('[[package]]\nname = \"wasm-bindgen\"\nversion = \"0.2.130\"\n')
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            (root / '.git/HEAD').write_text('1' * 40 + '\n')
            env = {**os.environ, 'CARGO_TARGET_DIR': str(root / 'ephemeral'),
                   'CYRUS_WEB_CHECKPOINT_DIR': str(root / 'retained'), 'CYRUS_WEB_KEEP_DEBUG': '1'}
            def run(expected):
                result = subprocess.run(['bash', str(scripts / 'build-web.sh')], cwd=root,
                                        env=env, capture_output=True, text=True)
                self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
            (root / 'fail-compile').touch()
            run(7)
            self.assertTrue((root / 'retained/cyrus_assets_worker.wasm.json').exists())
            shutil.rmtree(root / 'ephemeral')
            (root / 'fail-compile').unlink()
            (root / 'fail-package').touch()
            run(8)
            self.assertEqual((root / 'cargo.log').read_text().splitlines(),
                             ['cyrus-assets-worker', 'cyrus-web', 'cyrus-web'])
            shutil.rmtree(root / 'ephemeral')
            (root / 'fail-package').unlink()
            run(0)
            self.assertEqual(len((root / 'cargo.log').read_text().splitlines()), 3)
            record = json.loads((root / 'target/web/build-provenance.json').read_text())
            self.assertIn('target/web/pkg/cyrus-web_bg.wasm', record['artifacts'])


if __name__ == '__main__':
    unittest.main()
