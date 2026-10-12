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
echo "$*" >> cargo-calls.txt
profile=release
while [[ $# -gt 0 ]]; do
    case "$1" in
        --target) platform=$2; shift 2 ;;
        -p) apps+=("$2"); shift 2 ;;
        --profile) profile=$2; shift 2 ;;
        *) shift ;;
    esac
done
suffix=""
if [[ "$platform" = *-windows-* ]]; then suffix=.exe; fi
mkdir -p "target/$platform/$profile"
for app in "${apps[@]}"; do
    if [[ "$platform" = wasm32-unknown-unknown ]]; then
        binary=$app
        if [[ "$app" = cyrus-assets-worker ]]; then binary=cyrus_assets_worker; fi
        echo wasm > "target/$platform/$profile/$binary.wasm"
        continue
    fi
    echo executable > "target/$platform/release/$app$suffix"
    chmod +x "target/$platform/release/$app$suffix"
done
''')
        cargo.chmod(0o755)
        (scripts / "build-web.sh").write_text('''#!/bin/bash
set -euo pipefail
echo "$*" >> web-calls.txt
for app in "$@"; do
mkdir -p target/web/pkg cyrus-config/cache
mkdir -p target/web/pkg/snippets/module
echo helper > target/web/pkg/snippets/module/helper.js
echo private > cyrus-config/cache/private.glb
ln -sfn "$PWD/cyrus-config" target/web/cyrus-config
echo page > target/web/index.html
echo service > target/web/sw.js
for file in loader.js install.js loading.css game-assets.js asset-worker.js pkg/cyrus-assets-worker.js pkg/cyrus-assets-worker_bg.wasm; do echo code > "target/web/$file"; done
echo code > "target/web/pkg/$app.js"
echo wasm > "target/web/pkg/${app}_bg.wasm"
echo private > target/web/pkg/other.wasm
if [[ "$app" = cyrus-viewer ]]; then
    for file in viewer.html loader.js loading.css asset-worker.js pkg/cyrus-assets-worker.js pkg/cyrus-assets-worker_bg.wasm; do
        echo code > "target/web/$file"
    done
fi
done
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
                    "browser/loader.js", "browser/install.js", "browser/loading.css",
                    "browser/viewer-server.py", "browser/asset-worker.js", "browser/game-assets.js",
                    "browser/pkg/cyrus-assets-worker.js", "browser/pkg/cyrus-assets-worker_bg.wasm",
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
        self.assertEqual((self.root / "web-calls.txt").read_text().splitlines(),
                         ["cyrus-web", "cyrus-web cyrus-viewer"])

    def test_real_web_builder_compiles_and_exports_all_requested_apps(self):
        source = Path(__file__).parent
        for name in ("build-web.sh", "toolchain.sh"):
            shutil.copy2(source / name, self.root / "dev/scripts" / name)
        apps = ["cyrus-web"]
        if (source.parent.parent / "crates/cyrus-viewer/Cargo.toml").exists():
            apps.append("cyrus-viewer")
        for app in apps:
            web = self.root / "crates" / app / "web"
            web.mkdir()
            (web / "index.html").write_text('<meta name="cyrus-worlds" content="">')
            (web / "sw.js").write_text("service worker")
        web = self.root / "dev/web"
        web.mkdir()
        for name in ("loader.js", "install.js", "sw.js", "loading.css", "asset-worker.js", "game-assets.js"):
            (web / name).write_text(name)
        bindgen = self.root / "bin/wasm-bindgen"
        bindgen.write_text('''#!/bin/bash
set -euo pipefail
while [[ $# -gt 0 ]]; do
    case "$1" in
        --out-dir) out=$2; shift 2 ;;
        --out-name) app=$2; shift 2 ;;
        *.wasm) test -s "$1"; shift ;;
        *) shift ;;
    esac
done
mkdir -p "$out"
echo code > "$out/$app.js"
echo wasm > "$out/${app}_bg.wasm"
''')
        bindgen.chmod(0o755)
        subprocess.run(["bash", "dev/scripts/build-web.sh", *apps],
                       cwd=self.root, env=self.env, check=True)
        packages = "-p cyrus-assets-worker -p cyrus-web"
        if "cyrus-viewer" in apps:
            packages += " -p cyrus-viewer"
            self.assertTrue((self.root / "target/web/viewer.html").is_file())
        apps.append("cyrus-assets-worker")
        self.assertEqual((self.root / "cargo-calls.txt").read_text().splitlines(),
                         [f"build {packages} --target wasm32-unknown-unknown --profile wasm-release"])
        for app in apps:
            for suffix in (".js", "_bg.wasm"):
                self.assertTrue((self.root / f"target/web/pkg/{app}{suffix}").is_file())
        self.assertTrue((self.root / "target/web/index.html").is_file())

    def test_release_publishes_all_packages_and_skips_published_versions(self):
        workflow = (Path(__file__).parents[2] / ".github/workflows/release.yml").read_text()
        script = workflow.split("      - name: Create release with all packages and publish\n", 1)[1]
        script = script.split("        run: |\n", 1)[1]
        script = "\n".join(line[10:] for line in script.splitlines())
        gh = self.root / "bin/gh"
        gh.write_text('''#!/bin/bash
set -euo pipefail
if [[ "$1" = api ]]; then
    if [[ "$RELEASE_STATE" = missing ]]; then echo '[]';
    else printf '[{"tag_name":"v0.1.1","draft":%s}]\\n' "$RELEASE_STATE"; fi
else
    printf '%s\\n' "$*" >> release-calls.txt
fi
''')
        gh.chmod(0o755)
        artifacts = self.root / "artifacts"
        artifacts.mkdir()
        for name in ("native.tar.gz", "browser.tar.gz", "SHA256SUMS"):
            (artifacts / name).write_text(name)
        (self.root / "CHANGELOG.md").write_text("Release notes")
        env = dict(self.env, GH_REPO="michidk/cyrus", TAG_NAME="v0.1.1")
        for state in ("missing", "true", "false"):
            calls = self.root / "release-calls.txt"
            calls.unlink(missing_ok=True)
            subprocess.run(["bash", "-euo", "pipefail", "-c", script], cwd=self.root,
                           env=dict(env, RELEASE_STATE=state), check=True)
            commands = calls.read_text().splitlines() if calls.exists() else []
            if state == "false":
                self.assertEqual(commands, [])
                continue
            self.assertEqual(commands[-1], "release edit v0.1.1 --draft=false")
            for name in ("native.tar.gz", "browser.tar.gz", "SHA256SUMS"):
                self.assertIn(f"artifacts/{name}", commands[0])
            self.assertIn("--notes-file CHANGELOG.md", "\n".join(commands))

    def test_build_failure_produces_no_archive(self):
        (self.root / "bin/cargo").write_text("#!/bin/bash\nexit 7\n")
        with self.assertRaises(subprocess.CalledProcessError) as error:
            self.build("native", "x86_64-unknown-linux-gnu")
        self.assertEqual(error.exception.returncode, 7)
        self.assertEqual(list((self.root / "target/artifacts").iterdir()), [])


if __name__ == "__main__":
    unittest.main()
