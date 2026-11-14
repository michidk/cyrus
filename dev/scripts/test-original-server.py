#!/usr/bin/env python3
"""Read-only original-file hosting contracts; no native exporter participates."""
import functools
import http.client
import importlib.util
import json
from pathlib import Path
import tempfile
import threading
import unittest

spec = importlib.util.spec_from_file_location("original_server", Path(__file__).with_name("viewer-server.py"))
server = importlib.util.module_from_spec(spec)
spec.loader.exec_module(server)


class OriginalServerTest(unittest.TestCase):
    def test_cd_media_is_served_without_gog_companions(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            install = root / "Redguard"
            (install / "fxart").mkdir(parents=True)
            (install / "maps").mkdir()
            (install / "RGFX.EXE").write_bytes(b"synthetic executable")
            (root / "CDROM").mkdir()
            (root / "Soundtrack").mkdir()
            movie = b"synthetic movie frames"
            track = b"synthetic ripped audio"
            (root / "CDROM/INTRO.SMK").write_bytes(movie)
            (root / "Soundtrack/01 Track 1.mp3").write_bytes(track)
            (root / "Soundtrack/02 Track 2.MP3").write_bytes(b"second track")
            (root / "Soundtrack/cover.jpg").write_bytes(b"not audio")
            (root / "private").write_bytes(b"not selected media")
            (root / "CDROM/leak.smk").symlink_to(root / "private")
            paths, manifest = server.original_files(install)
            listing = {item["path"]: item for item in json.loads(manifest)["files"]}
            for name, payload in [("disc/INTRO.SMK", movie), ("disc/track02.mp3", track),
                                  ("disc/track03.mp3", b"second track")]:
                self.assertIn(name, listing)
                self.assertEqual(listing[name]["bytes"], len(payload))
            self.assertNotIn("disc/leak.smk", paths)
            self.assertNotIn("disc/cover.jpg", paths)
            self.assertNotIn("disc/game.gog", paths)
            handler = functools.partial(server.Handler, directory=str(root), originals=paths,
                                        manifest=manifest, install=install)
            service = server.Server(("127.0.0.1", 0), handler)
            thread = threading.Thread(target=service.serve_forever, daemon=True)
            thread.start()
            client = http.client.HTTPConnection("127.0.0.1", service.server_port, timeout=5)
            try:
                for name, payload in [("disc/INTRO.SMK", movie), ("disc/track02.mp3", track)]:
                    client.request("GET", "/original/" + name, headers={"Range": "bytes=2-8"})
                    response = client.getresponse()
                    self.assertEqual(response.status, 206)
                    self.assertEqual(response.read(), payload[2:9])
            finally:
                client.close()
                service.shutdown()
                service.server_close()
                thread.join()

    def test_manifest_ranges_and_unavailable_originals(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            install = root / "Redguard"
            (install / "fxart").mkdir(parents=True)
            (install / "maps").mkdir()
            (install / "RGFX.EXE").write_bytes(b"example game")
            (install / "fxart/test.rob").write_bytes(b"0123456789")
            (install / "maps/test.rgm").write_bytes(b"map")
            (install / "ENGLISH_wav").mkdir()
            (install / "ENGLISH_wav/#bon.wav").write_bytes(b"synthetic audio")
            (root / "game.gog").write_bytes(b"disc")
            (root / "DOSBOX").mkdir()
            (root / "DOSBOX/3DfxSpl2.dll").write_bytes(b"splash")
            web = root / "web"
            web.mkdir()
            paths, manifest = server.original_files(install)
            listing = json.loads(manifest)
            self.assertNotIn("cache", listing)
            self.assertIn("splash/3DfxSpl2.dll", paths)
            self.assertIn("disc/game.gog", paths)
            handler = functools.partial(server.Handler, directory=str(web), originals=paths,
                                        manifest=manifest, install=install, install_url="./original/")
            service = server.Server(("127.0.0.1", 0), handler)
            thread = threading.Thread(target=service.serve_forever, daemon=True)
            thread.start()
            client = http.client.HTTPConnection("127.0.0.1", service.server_port)
            try:
                client.request("GET", "/original/manifest.json")
                response = client.getresponse()
                self.assertIn("ENGLISH_wav/#bon.wav",
                              [file["path"] for file in json.loads(response.read())["files"]])
                client.request("GET", "/original/ENGLISH_wav/%23bon.wav?v=test")
                response = client.getresponse()
                self.assertEqual(response.status, 200)
                self.assertEqual(response.read(), b"synthetic audio")
                client.request("GET", "/launch.json")
                response = client.getresponse()
                self.assertEqual(json.loads(response.read()), {"install": "./original/"})
                client.request("GET", "/original/fxart/test.rob", headers={"Range": "bytes=3-6"})
                response = client.getresponse()
                self.assertEqual(response.status, 206)
                self.assertEqual(response.getheader("Content-Range"), "bytes 3-6/10")
                self.assertEqual(response.read(), b"3456")
                client.request("GET", "/original/fxart/test.rob", headers={"Range": "bytes=10-11"})
                response = client.getresponse()
                self.assertEqual(response.status, 416)
                response.read()
                for route in ("/original/../game.gog", "/game-config/cache/START.glb"):
                    client.request("GET", route)
                    response = client.getresponse()
                    self.assertEqual(response.status, 404)
                    response.read()
                (install / "RGFX.EXE").unlink()
                client.request("GET", "/original/manifest.json")
                response = client.getresponse()
                self.assertEqual(response.status, 404)
                response.read()
            finally:
                client.close()
                service.shutdown()
                service.server_close()
                thread.join()


if __name__ == "__main__":
    unittest.main()
