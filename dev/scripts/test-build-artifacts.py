#!/usr/bin/env python3
"""Check archive contents without compiling or reading a game install."""
import os
import json
from html.parser import HTMLParser
from urllib.parse import urlparse
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
import zipfile


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
if [[ "$1" = metadata ]]; then
    printf '{"target_directory":"%s","packages":[{"targets":[{"name":"different_name","kind":["cdylib"]}]}]}\\n' "${CARGO_TARGET_DIR:-$PWD/target}"
    exit 0
fi
profile=release
while [[ $# -gt 0 ]]; do
    case "$1" in
        --target) platform=$2; shift 2 ;;
        -p) apps+=("$2"); shift 2 ;;
        --manifest-path) apps+=(different_name); shift 2 ;;
        --profile) profile=$2; shift 2 ;;
        *) shift ;;
    esac
done
suffix=""
if [[ "$platform" = *-windows-* ]]; then suffix=.exe; fi
build_root="${CARGO_TARGET_DIR:-target}"
mkdir -p "$build_root/$platform/$profile"
for app in "${apps[@]}"; do
    if [[ "$platform" = wasm32-* ]]; then
        binary=$app
        if [[ "$app" = cyrus-assets-worker ]]; then binary=cyrus_assets_worker; fi
        echo wasm > "$build_root/$platform/$profile/$binary.wasm"
        continue
    fi
    echo executable > "$build_root/$platform/release/$app$suffix"
    chmod +x "$build_root/$platform/release/$app$suffix"
done
''')
        cargo.chmod(0o755)
        (scripts / "build-web.sh").write_text('''#!/bin/bash
set -euo pipefail
echo "$*" >> web-calls.txt
for app in "$@"; do
mkdir -p target/web/pkg game-config/cache
mkdir -p target/web/pkg/snippets/module
echo helper > target/web/pkg/snippets/module/helper.js
echo private > game-config/cache/private.glb
ln -sfn "$PWD/game-config" target/web/game-config
echo page > target/web/index.html
echo service > target/web/sw.js
for file in loader.js install.js local-install.js loading.css game-assets.js asset-worker.js pkg/cyrus-assets-worker.js pkg/cyrus-assets-worker_bg.wasm; do echo code > "target/web/$file"; done
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
        self.env.pop("CARGO_TARGET_DIR", None)

    def build(self, *args):
        subprocess.run(["bash", "dev/scripts/build-artifacts.sh", *args],
                       cwd=self.root, env=self.env, check=True)

    def members(self, archive):
        with tarfile.open(self.root / "target/artifacts" / archive) as package:
            return {member.name for member in package if member.isfile()}

    def test_native_platforms_and_optional_viewer(self):
        linux = "x86_64-unknown-linux-gnu"
        self.build("native", linux)
        self.assertEqual(self.members(f"cyrus-{linux}.tar.gz"), {"cyrus"})
        with tarfile.open(self.root / f"target/artifacts/cyrus-{linux}.tar.gz") as package:
            self.assertTrue(package.getmember("cyrus").mode & 0o111)
        self.assertFalse((self.root / f"target/artifacts/cyrus-viewer-{linux}.tar.gz").exists())
        (self.root / "crates/cyrus-viewer/Cargo.toml").touch()
        windows = "x86_64-pc-windows-msvc"
        stale = self.root / f"target/artifacts/cyrus-{windows}.exe"
        stale.write_bytes(b"old uncompressed download")
        self.build("native", windows)
        for app in ("cyrus", "cyrus-viewer"):
            with zipfile.ZipFile(self.root / f"target/artifacts/{app}-{windows}.zip") as archive:
                self.assertEqual(archive.namelist(), [f"{app}.exe"])
                self.assertEqual(archive.getinfo(f"{app}.exe").compress_type, zipfile.ZIP_DEFLATED)
            self.assertFalse((self.root / f"target/artifacts/{app}-{windows}.exe").exists())
        (self.root / "crates/cyrus-viewer/Cargo.toml").unlink()
        self.build("native", windows)
        self.assertFalse((self.root / f"target/artifacts/cyrus-viewer-{windows}.zip").exists())

    def test_raytracing_packages_preserve_standard_downloads(self):
        for target, extension in (("x86_64-unknown-linux-gnu", ".tar.gz"),
                                  ("x86_64-pc-windows-msvc", ".zip")):
            with self.subTest(target=target):
                (self.root / "crates/cyrus-viewer/Cargo.toml").touch()
                self.build("native", target)
                standard = self.root / f"target/artifacts/cyrus-{target}{extension}"
                viewer = self.root / f"target/artifacts/cyrus-viewer-{target}{extension}"
                originals = (standard.read_bytes(), viewer.read_bytes())
                self.build("native-raytracing", target)
                self.assertEqual((standard.read_bytes(), viewer.read_bytes()), originals)
                package = self.root / f"target/artifacts/cyrus-raytracing-{target}{extension}"
                if extension == ".zip":
                    with zipfile.ZipFile(package) as archive:
                        self.assertEqual(archive.namelist(), ["cyrus.exe"])
                        self.assertEqual(archive.getinfo("cyrus.exe").compress_type,
                                         zipfile.ZIP_DEFLATED)
                else:
                    self.assertEqual(self.members(package.name), {"cyrus"})
                command = (self.root / "cargo-calls.txt").read_text().splitlines()[-1]
                self.assertIn("--features cyrus/raytracing", command)
                self.assertNotIn("cyrus-viewer", command)
                self.assertNotIn("cyrus-launcher", command)

    def test_raytracing_failure_removes_stale_download(self):
        target = "x86_64-unknown-linux-gnu"
        self.build("native-raytracing", target)
        (self.root / "bin/cargo").write_text("#!/bin/bash\nexit 7\n")
        with self.assertRaises(subprocess.CalledProcessError):
            self.build("native-raytracing", target)
        self.assertFalse((self.root / f"target/artifacts/cyrus-raytracing-{target}.tar.gz").exists())

    def mod_fixtures(self):
        for name in ("guest", "shader", "manifest-only"):
            mod = self.root / "mods" / name
            mod.mkdir(parents=True)
            (mod / "mod.toml").write_text(f'name = "{name}"')
            (mod / "private.log").write_text("exclude")
        guest = self.root / "mods/guest"
        (guest / "Cargo.toml").write_text('[package]\nname = "different-name"\n')
        (guest / "src").mkdir()
        (guest / "src/lib.rs").write_text("exclude source")
        (self.root / "mods/shader/effect.wesl").write_text("shader")
        overlay = self.root / "mods/manifest-only/files/maps"
        overlay.mkdir(parents=True)
        (overlay / "custom.txt").write_text("authored overlay")

    def test_native_mod_bundles_include_only_runtime_files(self):
        self.mod_fixtures()
        for platform, suffix, extension in (
            ("x86_64-unknown-linux-gnu", "", "tar.gz"),
            ("x86_64-pc-windows-msvc", ".exe", "zip"),
        ):
            self.build("native-mods", platform)
            path = self.root / f"target/artifacts/cyrus-with-mods-{platform}.{extension}"
            if extension == "zip":
                with zipfile.ZipFile(path) as archive:
                    files = {n: archive.read(n) for n in archive.namelist() if not n.endswith("/")}
            else:
                with tarfile.open(path) as archive:
                    files = {m.name.removeprefix("./"): archive.extractfile(m).read()
                             for m in archive if m.isfile()}
            expected = {f"cyrus{suffix}", "README.txt",
                        "mods/guest/mod.toml", "mods/guest/mod.wasm",
                        "mods/shader/mod.toml", "mods/shader/effect.wesl",
                        "mods/manifest-only/mod.toml",
                        "mods/manifest-only/files/maps/custom.txt"}
            self.assertEqual(set(files), expected)
            self.assertEqual(files["mods/guest/mod.wasm"], b"wasm\n")
            self.assertTrue((self.root / f"target/artifacts/cyrus-{platform}{'.zip' if suffix else '.tar.gz'}").is_file())

    def test_native_mod_bundles_reject_external_overlay_links(self):
        self.mod_fixtures()
        private = self.root / "private-install.txt"
        private.write_text("local data must never enter a release")
        link = self.root / "mods/manifest-only/files/maps/external.txt"
        link.symlink_to(private)
        for platform, extension in (("x86_64-unknown-linux-gnu", "tar.gz"),
                                    ("x86_64-pc-windows-msvc", "zip")):
            with self.subTest(platform=platform):
                archive = self.root / f"target/artifacts/cyrus-with-mods-{platform}.{extension}"
                archive.parent.mkdir(parents=True, exist_ok=True)
                archive.write_text("stale package")
                result = subprocess.run(
                    ["bash", "dev/scripts/build-artifacts.sh", "native-mods", platform],
                    cwd=self.root, env=self.env, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("symbolic link", result.stderr)
                self.assertIn("mods/manifest-only/files/maps/external.txt", result.stderr)
                self.assertFalse(archive.exists())
                self.assertFalse(list(archive.parent.glob("package.*")))

    def test_mod_bundle_delivers_launcher_beside_game_on_each_platform(self):
        self.mod_fixtures()
        crate = self.root / "crates/cyrus-launcher"
        crate.mkdir()
        (crate / "Cargo.toml").touch()
        for platform, suffix in (
            ("x86_64-unknown-linux-gnu", ""),
            ("x86_64-apple-darwin", ""),
            ("aarch64-apple-darwin", ""),
            ("x86_64-pc-windows-msvc", ".exe"),
        ):
            with self.subTest(platform=platform):
                self.build("native-mods", platform)
                stem = self.root / f"target/artifacts/cyrus-with-mods-{platform}"
                if suffix:
                    with zipfile.ZipFile(str(stem) + ".zip") as archive:
                        files = set(archive.namelist())
                else:
                    with tarfile.open(str(stem) + ".tar.gz") as archive:
                        files = {m.name.removeprefix("./") for m in archive if m.isfile()}
                        for app in ("cyrus", "cyrus-launcher"):
                            self.assertTrue(archive.getmember(f"./{app}").mode & 0o111)
                self.assertIn(f"cyrus{suffix}", files)
                self.assertIn(f"cyrus-launcher{suffix}", files)
                self.assertNotIn("play-cyrus.cmd", files)
                self.assertNotIn("play-cyrus.sh", files)
                calls = (self.root / "cargo-calls.txt").read_text().splitlines()
                build = [call for call in calls if call.startswith(
                    f"build --locked --release --target {platform} ")]
                self.assertEqual(len(build), 1)
                self.assertIn("-p cyrus-launcher", build[0])

    def test_download_page_links_complete_launcher_bundles(self):
        class Links(HTMLParser):
            def __init__(self):
                super().__init__()
                self.hrefs = []

            def handle_starttag(self, tag, attrs):
                if tag == "a":
                    self.hrefs.extend(value for key, value in attrs if key == "href")

        page = Links()
        page.feed((Path(__file__).parents[2] / "dev/pages/index.html").read_text())
        bundles = [url for url in page.hrefs if "/download/cyrus-with-mods-" in url]
        self.assertEqual(len(bundles), 4)
        self.mod_fixtures()
        launcher = self.root / "crates/cyrus-launcher"
        launcher.mkdir()
        (launcher / "Cargo.toml").touch()
        for platform in ("x86_64-unknown-linux-gnu", "x86_64-apple-darwin",
                         "aarch64-apple-darwin", "x86_64-pc-windows-msvc"):
            with self.subTest(platform=platform):
                self.build("native-mods", platform)
                extension = ".zip" if "windows" in platform else ".tar.gz"
                name = f"cyrus-with-mods-{platform}{extension}"
                url = f"https://github.com/michidk/cyrus/releases/latest/download/{name}"
                self.assertIn(url, bundles)
                self.assertTrue((self.root / "target/artifacts" / Path(urlparse(url).path).name).is_file())

    def test_mod_bundle_uses_inherited_build_directory(self):
        self.mod_fixtures()
        # Fake Cargo writes tiny fixture files, never real compiler outputs.
        build_root = self.root / "shared-build"
        self.env["CARGO_TARGET_DIR"] = str(build_root)
        platform = "x86_64-unknown-linux-gnu"
        self.build("native-mods", platform)
        path = build_root / f"artifacts/cyrus-with-mods-{platform}.tar.gz"
        with tarfile.open(path) as archive:
            self.assertEqual(archive.extractfile("./mods/guest/mod.wasm").read(), b"wasm\n")
        self.assertFalse((self.root / "target").exists())

    def test_main_pushes_have_independent_workflow_groups(self):
        workflows = Path(__file__).parents[2] / ".github/workflows"
        for name, cancellation in (
            ("pr-artifacts.yml", "github.event_name == 'pull_request'"),
            ("ci.yml", "github.ref != 'refs/heads/main'"),
        ):
            with self.subTest(workflow=name):
                workflow = (workflows / name).read_text()
                concurrency = workflow.split("concurrency:\n", 1)[1].split("\n\n", 1)[0]
                self.assertIn("${{ github.event_name == 'push' && github.sha || github.ref }}", concurrency)
                self.assertIn("cancel-in-progress: ${{ " + cancellation + " }}", concurrency)

    def test_native_workflows_build_and_upload_mod_bundles(self):
        for name in ("pr-artifacts.yml", "build-release.yml"):
            workflow = (Path(__file__).parents[2] / ".github/workflows" / name).read_text()
            native = workflow.split("  native:\n", 1)[1].split("\n  web:", 1)[0]
            self.assertIn("targets: ${{ matrix.target }}, wasm32-wasip2", native)
            self.assertIn('build-artifacts.sh native-mods "$BUILD_TARGET"', native)
            self.assertIn("target/artifacts/cyrus-with-mods-${{ matrix.target }}*", native)
            self.assertIn('build-artifacts.sh native-raytracing "$BUILD_TARGET"', native)
            self.assertIn('smoke-native-artifacts.py target/artifacts "$BUILD_TARGET" --raytracing', native)
            self.assertIn("target/artifacts/cyrus-raytracing-${{ matrix.target }}*", native)
            tracing_steps = native.split("      - name: Build and package ray-tracing game\n", 1)[1]
            tracing_steps = tracing_steps.split("      - uses: actions/upload-artifact@", 1)[0]
            # PR runs add an event condition after the platform test, so match the platform test itself.
            self.assertEqual(tracing_steps.count("runner.os == 'Linux' || runner.os == 'Windows'"), 2)

    def test_pr_standard_artifacts_publish_before_optional_raytracing(self):
        workflow = (Path(__file__).parents[2] / ".github/workflows/pr-artifacts.yml").read_text()
        native = workflow.split("  native:\n", 1)[1].split("\n  web:", 1)[0]
        standard_smoke = native.index("- name: Smoke packaged native executables")
        standard_name = native.index("name: native-${{ matrix.target }}")
        tracing_build = native.index("- name: Build and package ray-tracing game")
        tracing_smoke = native.index("- name: Smoke packaged ray-tracing game")
        tracing_name = native.index("name: native-raytracing-${{ matrix.target }}")
        self.assertLess(standard_smoke, standard_name)
        self.assertLess(standard_name, tracing_build)
        self.assertLess(tracing_build, tracing_smoke)
        self.assertLess(tracing_smoke, tracing_name)
        standard = native[standard_name:tracing_build]
        self.assertNotIn("cyrus-raytracing", standard)
        for package in ("cyrus-", "cyrus-with-mods-", "cyrus-viewer-"):
            self.assertIn("target/artifacts/" + package + "${{ matrix.target }}*", standard)
        tracing_upload = native[tracing_smoke:].split("      - uses: actions/upload-artifact@", 1)[1]
        self.assertIn("github.event_name == 'pull_request'", tracing_upload)
        self.assertIn("runner.os == 'Linux' || runner.os == 'Windows'", tracing_upload)
        self.assertIn("path: target/artifacts/cyrus-raytracing-${{ matrix.target }}*", tracing_upload)
        self.assertEqual(native.count('build-artifacts.sh native-mods "$BUILD_TARGET"'), 1)
        self.assertEqual(native.count('build-artifacts.sh native-raytracing "$BUILD_TARGET"'), 1)

    def test_mod_build_failure_does_not_leave_a_previous_bundle(self):
        self.mod_fixtures()
        platform = "x86_64-unknown-linux-gnu"
        self.build("native-mods", platform)
        cargo = self.root / "bin/cargo"
        cargo.write_text(cargo.read_text().replace('profile=release',
                          'if [[ "$*" = *wasm32-wasip2* ]]; then exit 7; fi\nprofile=release'))
        with self.assertRaises(subprocess.CalledProcessError):
            self.build("native-mods", platform)
        self.assertFalse((self.root / f"target/artifacts/cyrus-with-mods-{platform}.tar.gz").exists())

    def test_browser_excludes_install_cache_and_stale_outputs(self):
        expected = {"browser/index.html", "browser/sw.js", "browser/README.md",
                    "browser/loader.js", "browser/install.js", "browser/local-install.js", "browser/loading.css",
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
        for name in ("build-web.sh", "toolchain.sh", "build-provenance.py", "resolve-wasm-bindgen.py"):
            shutil.copy2(source / name, self.root / "dev/scripts" / name)
        subprocess.run(["git", "-c", "core.hooksPath=/dev/null", "init", "--quiet"],
                       cwd=self.root, check=True)
        subprocess.run(["git", "-c", "core.hooksPath=/dev/null",
                        "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                        "commit", "--quiet",
                        "--allow-empty", "-m", "fixture"], cwd=self.root, check=True)
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
        for name in ("loader.js", "install.js", "local-install.js", "sw.js", "loading.css", "asset-worker.js", "game-assets.js"):
            (web / name).write_text(name)
        (self.root / "Cargo.lock").write_text('[[package]]\nname = "wasm-bindgen"\nversion = "0.2.100"\n')
        bindgen = self.root / "bin/wasm-bindgen"
        bindgen.write_text('''#!/bin/bash
set -euo pipefail
if [[ "${1:-}" = --version ]]; then
    echo 'wasm-bindgen 0.2.100'
    exit 0
fi
while [[ $# -gt 0 ]]; do
    case "$1" in
        --out-dir) out=$2; shift 2 ;;
        --out-name) app=$2; shift 2 ;;
        *.wasm) test -s "$1"; shift ;;
        *) shift ;;
    esac
done
mkdir -p "$out"
mkdir -p "$out/snippets/$app"
echo snippet > "$out/snippets/$app/helper.js"
echo code > "$out/$app.js"
echo wasm > "$out/${app}_bg.wasm"
''')
        bindgen.chmod(0o755)
        subprocess.run(["bash", "dev/scripts/build-web.sh", *apps],
                       cwd=self.root, env=self.env, check=True)
        record = json.loads((self.root / "target/web/build-provenance.json").read_text())
        for app in [*apps, "cyrus-assets-worker"]:
            self.assertIn(f"target/web/pkg/snippets/{app}/helper.js", record["artifacts"])
        subprocess.run(["python3", "dev/scripts/build-provenance.py", "verify",
                        "target/web/build-provenance.json"], cwd=self.root, check=True)
        packages = "-p cyrus-assets-worker -p cyrus-web"
        if "cyrus-viewer" in apps:
            packages += " -p cyrus-viewer"
            self.assertTrue((self.root / "target/web/viewer.html").is_file())
        apps.append("cyrus-assets-worker")
        self.assertEqual((self.root / "cargo-calls.txt").read_text().splitlines(),
                         [f"build {packages} --target wasm32-unknown-unknown --profile wasm-dev"])
        for app in apps:
            for suffix in (".js", "_bg.wasm"):
                self.assertTrue((self.root / f"target/web/pkg/{app}{suffix}").is_file())
        self.assertTrue((self.root / "target/web/index.html").is_file())

    def test_build_failure_produces_no_archive(self):
        (self.root / "bin/cargo").write_text("#!/bin/bash\nexit 7\n")
        with self.assertRaises(subprocess.CalledProcessError) as error:
            self.build("native", "x86_64-unknown-linux-gnu")
        self.assertEqual(error.exception.returncode, 7)
        self.assertEqual(list((self.root / "target/artifacts").iterdir()), [])


if __name__ == "__main__":
    unittest.main()
