#!/usr/bin/env python3
"""Serve the local browser export with byte ranges for streamed movie audio."""
import argparse
import email.utils
import functools
import http.server
import os
import re
from pathlib import Path


class ExportHandler(http.server.SimpleHTTPRequestHandler):
    def send_head(self):
        self.remaining = None
        value = self.headers.get("Range")
        path = Path(self.translate_path(self.path))
        if not value or not path.is_file():
            return super().send_head()
        try:
            source = path.open("rb")
        except OSError:
            return super().send_head()
        stat = os.fstat(source.fileno())
        modified = email.utils.formatdate(stat.st_mtime, usegmt=True)
        if self.headers.get("If-Range", modified) != modified:
            source.close()
            return super().send_head()
        match = re.fullmatch(r"bytes=(\d*)-(\d*)", value.strip())
        if not match or not any(match.groups()):
            source.close()
            return super().send_head()
        first, last = match.groups()
        size = stat.st_size
        try:
            if first:
                start = int(first)
                end = min(int(last), size - 1) if last else size - 1
            else:
                start = max(0, size - int(last))
                end = size - 1
        except ValueError:
            source.close()
            return super().send_head()
        if start > end or start >= size:
            source.close()
            self.send_response(416)
            self.send_header("Content-Range", f"bytes */{size}")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return None
        source.seek(start)
        self.remaining = end - start + 1
        self.send_response(206)
        self.send_header("Content-Type", self.guess_type(str(path)))
        self.send_header("Content-Length", str(self.remaining))
        self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        self.send_header("Last-Modified", modified)
        self.end_headers()
        return source

    def end_headers(self):
        self.send_header("Accept-Ranges", "bytes")
        super().end_headers()

    def copyfile(self, source, output):
        if self.remaining is None:
            return super().copyfile(source, output)
        while self.remaining:
            data = source.read(min(self.remaining, 64 * 1024))
            if not data:
                break
            output.write(data)
            self.remaining -= len(data)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=36082)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--directory", default="target/web")
    args = parser.parse_args()
    handler = functools.partial(ExportHandler, directory=args.directory)
    with http.server.ThreadingHTTPServer((args.bind, args.port), handler) as server:
        server.serve_forever()
