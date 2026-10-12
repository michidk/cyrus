"""Original-file hosting contracts using synthetic bytes only."""
import importlib.util
import json
from unittest.mock import patch
import sys
import tempfile
import threading
import unittest
from functools import partial
from http.server import ThreadingHTTPServer
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen

sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("viewer_server", Path(__file__).with_name("viewer-server.py"))
server = importlib.util.module_from_spec(spec)
spec.loader.exec_module(server)


class Hosting(unittest.TestCase):
    def test_read_only_originals_and_ranges(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            install = root / "install"
            install.mkdir()
            (install / "fxart").mkdir()
            (install / "maps").mkdir()
            (install / "RGFX.EXE").write_bytes(b"0123456789")
            secret = root / "private.txt"
            secret.write_text("private")
            (install / "linked.txt").symlink_to(secret)
            paths, manifest = server.original_files(install)
            self.assertNotIn("linked.txt", paths)
            web = root / "web"
            web.mkdir()
            (web / "viewer.html").write_text("viewer")
            http = ThreadingHTTPServer(("127.0.0.1", 0), partial(
                server.Handler, directory=str(web), originals=paths, manifest=manifest))
            thread = threading.Thread(target=http.serve_forever)
            thread.start()
            base = f"http://127.0.0.1:{http.server_port}"
            try:
                with urlopen(base + "/") as response:
                    self.assertEqual(response.read(), b"viewer")
                with urlopen(base + "/launch.json") as response:
                    self.assertEqual(json.load(response), {"install": None})
                with urlopen(base + "/original/manifest.json") as response:
                    self.assertEqual(response.read(), manifest)
                    self.assertEqual(response.headers["Access-Control-Allow-Origin"], "*")
                with urlopen(Request(base + "/original/RGFX.EXE?range=2-5", headers={"Range": "bytes=2-5"})) as response:
                    self.assertEqual(response.status, 206)
                    self.assertEqual(response.headers["Content-Range"], "bytes 2-5/10")
                    self.assertEqual(response.read(), b"2345")
                for path, headers, status in [
                    ("/original/RGFX.EXE", {"Range": "bytes=99-100"}, 416),
                    ("/original/../private.txt", {}, 404),
                    ("/original/linked.txt", {}, 404),
                ]:
                    with self.assertRaises(HTTPError) as error:
                        urlopen(Request(base + path, headers=headers))
                    self.assertEqual(error.exception.code, status)
                self.assertEqual((install / "RGFX.EXE").read_bytes(), b"0123456789")
            finally:
                http.shutdown()
                thread.join()
                http.server_close()

    def test_game_preparation_is_serial_and_outside_install(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            install = root / "original"
            install.mkdir()
            with self.assertRaises(ValueError):
                server.GameSource(install, b"manifest", root / "cyrus", install / "cache")
            source = server.GameSource(install, b"manifest", root / "cyrus", root / "work")
            calls = []
            def export(*args):
                calls.append(args)
                source.cache.mkdir(parents=True, exist_ok=True)
                (source.cache / "START.glb").write_bytes(b"scene")
            with patch.object(source, "export", export):
                self.assertEqual(source.prepare("START.glb").read_bytes(), b"scene")
                source.prepare("START.player")
                source.prepare("START.player-models/CYRUS.glb")
                source.prepare("hud/display.hud")
                self.assertEqual(calls, [("--export-startup",), ("--export-scene", "--scene", "START")])
                with self.assertRaises(ValueError):
                    source.prepare("../secret")
                with self.assertRaises(ValueError):
                    source.prepare("/absolute")
                self.assertEqual(list(install.iterdir()), [])

    def test_shared_default_and_generation_cache_routes(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            install = root / "install"
            install.mkdir()
            source = server.GameSource(install, b"manifest", None, root / "work")
            source.cache.mkdir(parents=True)
            (source.cache / "START.glb").write_bytes(b"prepared scene")
            http = server.Server(("127.0.0.1", 0), partial(
                server.Handler, directory=str(root), originals={}, manifest=b"{}",
                source=source, install_url="./original/", page="index.html"))
            thread = threading.Thread(target=http.serve_forever)
            thread.start()
            base = f"http://127.0.0.1:{http.server_port}"
            try:
                with urlopen(base + "/launch.json") as response:
                    self.assertEqual(json.load(response), {"install": "./original/"})
                with patch.object(source, "export"):
                    with urlopen(base + f"/cyrus-config/cache/{source.generation}/START.glb") as response:
                        self.assertEqual(response.read(), b"prepared scene")
                    with urlopen(Request(base + f"/cyrus-config/cache/{source.generation}/START.glb", headers={"Range": "bytes=2-5"})) as response:
                        self.assertEqual(response.status, 206)
                        self.assertEqual(response.headers["Content-Range"], "bytes 2-5/14")
                        self.assertEqual(response.read(), b"epar")
                    with urlopen(Request(base + f"/cyrus-config/cache/{source.generation}/START.glb", headers={"Range": "bytes=-5"}, method="HEAD")) as response:
                        self.assertEqual(response.status, 206)
                        self.assertEqual(response.headers["Content-Range"], "bytes 9-13/14")
                        self.assertEqual(response.read(), b"")
                with self.assertRaises(HTTPError) as error:
                    urlopen(base + "/cyrus-config/cache/START.glb")
                self.assertEqual(error.exception.code, 404)
                with self.assertRaises(HTTPError) as error:
                    urlopen(base + "/original/manifest.json")
                self.assertEqual(error.exception.code, 404, "removed installs cannot be reused")
            finally:
                http.shutdown()
                thread.join()
                http.server_close()

    def test_invalid_explicit_install_does_not_fall_back(self):
        with tempfile.TemporaryDirectory() as directory:
            with self.assertRaises(ValueError):
                server.locate_install(Path(directory))


if __name__ == "__main__":
    unittest.main()
