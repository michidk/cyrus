#!/usr/bin/env python3
"""Exercise exported audio ranges through the HTTP interface."""
import contextlib
import functools
import http.client
import http.server
import importlib.util
from pathlib import Path
import tempfile
import threading
import unittest

spec = importlib.util.spec_from_file_location("web_server", Path(__file__).with_name("web-server.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ExportServerTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        Path(cls.directory.name, "test.wav").write_bytes(b"0123456789")
        Path(cls.directory.name, "empty.wav").write_bytes(b"")
        handler = functools.partial(module.ExportHandler, directory=cls.directory.name)
        cls.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), handler)
        cls.worker = threading.Thread(target=cls.server.serve_forever)
        cls.worker.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()
        cls.worker.join()
        cls.directory.cleanup()

    def request(self, headers, path="/test.wav", method="GET"):
        with contextlib.closing(http.client.HTTPConnection("127.0.0.1", self.server.server_port)) as connection:
            connection.request(method, path, headers=headers)
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), response.read()

    def test_audio_ranges(self):
        for value, content, span in [("bytes=0-3", b"0123", "bytes 0-3/10"),
                                     ("bytes=6-", b"6789", "bytes 6-9/10"),
                                     ("bytes=-3", b"789", "bytes 7-9/10"),
                                     ("bytes=8-100", b"89", "bytes 8-9/10")]:
            with self.subTest(value=value):
                status, headers, data = self.request({"Range": value})
                self.assertEqual(status, 206)
                self.assertEqual(data, content)
                self.assertEqual(headers["Content-Range"], span)
                self.assertEqual(int(headers["Content-Length"]), len(content))

    def test_unsatisfiable_and_empty_ranges(self):
        for value, path, span in [("bytes=10-", "/test.wav", "bytes */10"),
                                  ("bytes=6-3", "/test.wav", "bytes */10"),
                                  ("bytes=-0", "/test.wav", "bytes */10"),
                                  ("bytes=0-", "/empty.wav", "bytes */0")]:
            status, headers, data = self.request({"Range": value}, path)
            self.assertEqual((status, data), (416, b""))
            self.assertEqual(headers["Content-Range"], span)

    def test_full_download_and_changed_validator(self):
        for headers in [{}, {"Range": "bytes=0-3", "If-Range": "old"},
                        {"Range": "bytes=0-2,6-8"}, {"Range": "bytes=broken"}, {"Range": "bytes=" + "9" * 5000 + "-"}]:
            self.assertEqual(self.request(headers)[::2], (200, b"0123456789"))

    def test_head_has_no_body(self):
        status, headers, data = self.request({"Range": "bytes=2-4"}, method="HEAD")
        self.assertEqual((status, data), (206, b""))
        self.assertEqual(headers["Content-Length"], "3")


if __name__ == "__main__":
    unittest.main()
