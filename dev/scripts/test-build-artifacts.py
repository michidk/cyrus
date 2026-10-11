#!/usr/bin/env python3
"""Check archive contents without compiling or reading a game install."""
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


class ArtifactTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        scripts = self.root / "dev/scripts"
        scripts.mkdir(parents=True)
        source = Path(__file__).parent
        for name in ("build-artifacts.sh", "cargo.sh"):
            shutil.copy2(source / name, scripts / name)
        for app in ("cyrus", "cyrus-web", "cyrus-viewer"):
            crate = self.root / "crates" / app
            crate.mkdir(parents=True)
            (crate / "README.md").write_text(app)
        bindir = self.root / "bin"
        bindir.mkdir()
        cargo = bindir / "cargo"
        cargo.write_text('''#!/bin/bash
set -euo pipefail
while [[ $# -gt 0 ]]; do
    case "$1" in
        --target) platform=$2; shift 2 ;;
        -p) apps+=("$2"); shift 2 ;;
        *) shift ;;
    esac
done
suffix=""
if [[ "$platform" = *-windows-* ]]; then suffix=.exe; fi
mkdir -p "target/$platform/release"
for app in "${apps[@]}"; do
    echo executable > "target/$platform/release/$app$suffix"
    chmod +x "target/$platform/release/$app$suffix"
done
''')
        cargo.chmod(0o755)
        (scripts / "build-web.sh").write_text('''#!/bin/bash
set -euo pipefail
mkdir -p target/web/pkg cyrus-config/cache
mkdir -p target/web/pkg/snippets/module
echo helper > target/web/pkg/snippets/module/helper.js
echo private > cyrus-config/cache/private.glb
ln -sfn "$PWD/cyrus-config" target/web/cyrus-config
echo page > target/web/index.html
echo service > target/web/sw.js
echo code > "target/web/pkg/$1.js"
echo wasm > "target/web/pkg/${1}_bg.wasm"
echo private > target/web/pkg/other.wasm
if [[ "$1" = cyrus-viewer ]]; then
    for file in viewer.html loader.js loading.css asset-worker.js pkg/cyrus-assets-worker.js pkg/cyrus-assets-worker_bg.wasm; do
        echo code > "target/web/$file"
    done
fi
''')
        (scripts / "viewer-server.py").write_text("# Local viewer server\n")
        self.env = dict(os.environ, PATH=f"{bindir}:/usr/bin:/bin")

    def build(self, *args):
        subprocess.run(["bash", "dev/scripts/build-artifacts.sh", *args],
                       cwd=self.root, env=self.env, check=True)

    def members(self, archive):
        with tarfile.open(self.root / "target/artifacts" / archive) as package:
            return {member.name for member in package if member.isfile()}

    def test_native_platforms_and_optional_viewer(self):
        linux = "x86_64-unknown-linux-gnu"
        self.build("native", linux)
        self.assertEqual(self.members(f"cyrus-{linux}.tar.gz"),
                         {"cyrus/cyrus", "cyrus/README.md"})
        with tarfile.open(self.root / f"target/artifacts/cyrus-{linux}.tar.gz") as package:
            self.assertTrue(package.getmember("cyrus/cyrus").mode & 0o111)
        self.assertFalse((self.root / f"target/artifacts/cyrus-viewer-{linux}.tar.gz").exists())
        (self.root / "crates/cyrus-viewer/Cargo.toml").touch()
        windows = "x86_64-pc-windows-msvc"
        self.build("native", windows)
        for app in ("cyrus", "cyrus-viewer"):
            self.assertEqual(self.members(f"{app}-{windows}.tar.gz"),
                             {f"{app}/{app}.exe", f"{app}/README.md"})
        (self.root / "crates/cyrus-viewer/Cargo.toml").unlink()
        self.build("native", windows)
        self.assertFalse((self.root / f"target/artifacts/cyrus-viewer-{windows}.tar.gz").exists())

    def test_browser_excludes_install_cache_and_stale_outputs(self):
        expected = {"browser/index.html", "browser/sw.js", "browser/README.md",
                    "browser/pkg/cyrus-web.js", "browser/pkg/cyrus-web_bg.wasm",
                    "browser/pkg/snippets/module/helper.js"}
        self.build("web")
        self.assertEqual(self.members("cyrus-browser.tar.gz"), expected)

        (self.root / "crates/cyrus-viewer/Cargo.toml").touch()
        self.build("web")
        expected.update(f"browser/{name}" for name in (
            "viewer.html", "loader.js", "loading.css", "asset-worker.js", "VIEWER.md", "viewer-server.py",
            "pkg/cyrus-viewer.js", "pkg/cyrus-viewer_bg.wasm",
            "pkg/cyrus-assets-worker.js", "pkg/cyrus-assets-worker_bg.wasm"))
        self.assertEqual(self.members("cyrus-browser.tar.gz"), expected)

    def test_build_failure_produces_no_archive(self):
        (self.root / "bin/cargo").write_text("#!/bin/bash\nexit 7\n")
        with self.assertRaises(subprocess.CalledProcessError) as error:
            self.build("native", "x86_64-unknown-linux-gnu")
        self.assertEqual(error.exception.returncode, 7)
        self.assertEqual(list((self.root / "target/artifacts").iterdir()), [])


if __name__ == "__main__":
    unittest.main()
