"""Original-file hosting contracts using synthetic bytes only."""
import importlib.util
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
                with urlopen(base + "/original/manifest.json") as response:
                    self.assertEqual(response.read(), manifest)
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


if __name__ == "__main__":
    unittest.main()
