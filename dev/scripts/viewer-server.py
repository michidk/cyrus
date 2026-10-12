#!/usr/bin/env python3
"""Serve viewer code and read-only original bytes; all decoding runs in the browser."""
import argparse
import hashlib
import io
import json
import re
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlsplit


def original_files(install):
    install = install.resolve(strict=True)
    for required in ("fxart", "maps", "RGFX.EXE"):
        if not (install / required).exists():
            raise ValueError(f"{install}: missing {required}")
    paths = {
        p.relative_to(install).as_posix(): p
        for p in install.rglob("*")
        if p.is_file() and not p.is_symlink() and p.resolve().is_relative_to(install)
    }
    for name in ("game.gog", "game.ins"):
        path = install.parent / name
        if path.is_file():
            paths[f"disc/{name}"] = path
    for path in (install / "glide2x_emu.ovl", install.parent / "DOSBOX/glide2x_emu.ovl"):
        if path.is_file():
            paths["splash/glide2x_emu.ovl"] = path
            break
    files = []
    for name, path in sorted(paths.items()):
        with path.open("rb") as source:
            digest = hashlib.file_digest(source, "sha256").hexdigest()
        files.append(dict(path=name, bytes=path.stat().st_size, digest=digest))
    return paths, json.dumps(dict(files=files)).encode()


def byte_range(header, size):
    match = re.fullmatch(r"bytes=(\d+)-(\d*)", header)
    if not match:
        raise ValueError("expected a single explicit byte range")
    start = int(match[1])
    end = min(int(match[2]) if match[2] else size - 1, size - 1)
    if not 0 <= start <= end < size:
        raise ValueError("range outside file")
    return start, end


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, originals, manifest, **kwargs):
        self.originals = originals
        self.manifest = manifest
        self.remaining = None
        super().__init__(*args, **kwargs)

    def list_directory(self, path):
        self.send_error(404)
        return None

    def send_head(self):
        self.remaining = None
        path = unquote(urlsplit(self.path).path)
        if path == "/":
            self.path = "/viewer.html"
        if not path.startswith("/original/"):
            return super().send_head()
        name = path.removeprefix("/original/")
        if name == "manifest.json":
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("Content-Length", str(len(self.manifest)))
            self.end_headers()
            return io.BytesIO(self.manifest)
        original = self.originals.get(name)
        if original is None:
            self.send_error(404)
            return None
        source = original.open("rb")
        size = original.stat().st_size
        start, end = 0, size - 1
        if "Range" in self.headers:
            try:
                start, end = byte_range(self.headers["Range"], size)
            except ValueError:
                source.close()
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{size}")
                self.send_header("Content-Length", "0")
                self.end_headers()
                return None
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        else:
            self.send_response(200)
        self.remaining = end - start + 1
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(self.remaining))
        self.end_headers()
        source.seek(start)
        return source

    def copyfile(self, source, output):
        if self.remaining is None:
            return super().copyfile(source, output)
        while self.remaining:
            chunk = source.read(min(self.remaining, 1024 * 1024))
            if not chunk:
                break
            output.write(chunk)
            self.remaining -= len(chunk)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--install", type=Path, required=True)
    parser.add_argument("--directory", type=Path, default=Path("target/web"))
    parser.add_argument("--port", type=int, required=True)
    args = parser.parse_args()
    originals, manifest = original_files(args.install)
    handler = partial(Handler, directory=str(args.directory), originals=originals, manifest=manifest)
    print(f"Serving {len(originals)} original files; decoding runs in the browser", flush=True)
    ThreadingHTTPServer(("127.0.0.1", args.port), handler).serve_forever()


if __name__ == "__main__":
    main()
