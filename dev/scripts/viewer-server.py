#!/usr/bin/env python3
"""Serve game/viewer code and a read-only install, with native game preparation."""
import argparse
import hashlib
import io
import json
import re
import os
import shutil
import subprocess
import threading
import socket
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
    match = re.fullmatch(r"bytes=(\d*)-(\d*)", header.strip())
    if not match or not any(match.groups()):
        raise ValueError("expected a single byte range")
    first, last = match.groups()
    start = int(first) if first else max(0, size - int(last))
    end = min(int(last), size - 1) if first and last else size - 1
    if not 0 <= start <= end < size:
        raise ValueError("range outside file")
    return start, end


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, originals, manifest, source=None, page="viewer.html", install_url=None, **kwargs):
        self.originals = originals
        self.manifest = manifest
        self.remaining = None
        self.source = source
        self.page = page
        self.install_url = install_url
        super().__init__(*args, **kwargs)

    def list_directory(self, path):
        self.send_error(404)
        return None

    def send_head(self):
        self.remaining = None
        path = unquote(urlsplit(self.path).path)
        if path == "/":
            self.path = "/" + self.page
        if path == "/launch.json":
            return self.json_response(dict(install=self.install_url))
        if path.startswith("/cyrus-config/cache/"):
            if self.source is None:
                self.send_error(404, "No local install configured")
                return None
            prefix = f"/cyrus-config/cache/{self.source.generation}/"
            if not path.startswith(prefix):
                self.send_error(404, "Unknown install cache")
                return None
            name = path.removeprefix(prefix)
            try:
                target = self.source.prepare(name)
            except (ValueError, OSError, subprocess.SubprocessError) as error:
                self.send_error(503, str(error))
                return None
            if not target.is_file():
                self.send_error(404)
                return None
            stat = target.stat()
            modified = self.date_time_string(stat.st_mtime)
            start, end = 0, stat.st_size - 1
            partial = "Range" in self.headers and self.headers.get("If-Range", modified) == modified
            if partial:
                try:
                    start, end = byte_range(self.headers["Range"], stat.st_size)
                except ValueError:
                    self.send_response(416)
                    self.send_header("Content-Range", f"bytes */{stat.st_size}")
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    return None
            self.remaining = end - start + 1
            self.send_response(206 if partial else 200)
            if partial:
                self.send_header("Content-Range", f"bytes {start}-{end}/{stat.st_size}")
            self.send_header("Accept-Ranges", "bytes")
            self.send_header("Content-Type", self.guess_type(str(target)))
            self.send_header("Content-Length", str(self.remaining))
            self.send_header("Last-Modified", modified)
            self.end_headers()
            stream = target.open("rb")
            stream.seek(start)
            return stream
        if not path.startswith("/original/"):
            return super().send_head()
        name = path.removeprefix("/original/")
        if name == "manifest.json":
            for required in ("fxart", "maps", "RGFX.EXE"):
                if self.source and not (self.source.install / required).exists():
                    self.send_error(404, "Game files are no longer available")
                    return None
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Content-Length", str(len(self.manifest)))
            self.end_headers()
            return io.BytesIO(self.manifest)
        original = self.originals.get(name)
        if original is None:
            self.send_error(404)
            return None
        try:
            source = original.open("rb")
        except OSError:
            self.send_error(404, "Game file is no longer available")
            return None
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

    def end_headers(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Expose-Headers", "Content-Length, Content-Range, Last-Modified")
        super().end_headers()

    def json_response(self, value):
        body = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Cache-Control", "no-store")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        return io.BytesIO(body)

    def copyfile(self, source, output):
        if self.remaining is None:
            return super().copyfile(source, output)
        while self.remaining:
            chunk = source.read(min(self.remaining, 1024 * 1024))
            if not chunk:
                break
            output.write(chunk)
            self.remaining -= len(chunk)


def locate_install(explicit=None):
    configured = explicit or os.environ.get("REDGUARD_DIR")
    if configured:
        path = Path(configured).resolve(strict=True)
        for required in ("fxart", "maps", "RGFX.EXE"):
            if not (path / required).exists():
                raise ValueError(f"{path}: missing {required}")
        return path
    candidates = [Path("redguard"), Path("Redguard"),
                  Path.home() / "GOG Games/Redguard", Path.home() / "GOG Games/Redguard/app/Redguard"]
    if os.environ.get("SystemDrive"):
        candidates.append(Path(os.environ["SystemDrive"] + "/GOG Games/Redguard"))
    return next((path.resolve() for path in candidates
                 if all((path / required).exists() for required in ("fxart", "maps", "RGFX.EXE"))), None)


class GameSource:
    """Prepare game assets outside the install using the existing native exporter."""
    def __init__(self, install, manifest, exporter, work):
        install = install.resolve()
        work = work.resolve()
        if work.is_relative_to(install):
            raise ValueError("Game cache must be outside the original install")
        self.install = install
        self.exporter = exporter
        self.generation = hashlib.sha256(str(install).encode() + manifest).hexdigest()
        self.work = work / self.generation
        self.cache = self.work / "cyrus-config/cache"
        self.lock = threading.Lock()
        self.ready = False
        self.scenes = set()

    def export(self, *args):
        if not self.exporter:
            raise ValueError("Game preparation needs --exporter /path/to/cyrus (build the native game first)")
        self.work.mkdir(parents=True, exist_ok=True)
        env = dict(os.environ)
        env.setdefault("RUST_MIN_STACK", "8388608")
        result = subprocess.run([str(self.exporter), "--install", str(self.install), *args],
                                cwd=self.work, env=env, capture_output=True, text=True, timeout=600)
        if result.returncode:
            raise ValueError("Game preparation failed: " + result.stderr[-2000:])

    def prepare(self, name):
        parts = Path(name).parts
        if not parts or any(part in ("..", ".") for part in parts) or Path(name).is_absolute():
            raise ValueError("Invalid cache path")
        target = self.cache / name
        if not target.resolve().is_relative_to(self.cache.resolve()):
            raise ValueError("Cache path outside generated files")
        with self.lock:
            if not self.ready:
                self.export("--export-startup")
                self.ready = True
            scene = parts[0].split(".")[0]
            if re.fullmatch(r"[A-Za-z0-9_]+", scene) and Path(parts[0]).suffix in (".glb", ".animations", ".environment", ".player", ".scripts", ".player-models") and scene not in self.scenes:
                self.export("--export-scene", "--scene", scene)
                self.scenes.add(scene)
        return target


class Server(ThreadingHTTPServer):
    # Startup images arrive in a burst while native asset preparation is running.
    request_queue_size = socket.SOMAXCONN


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    location = parser.add_mutually_exclusive_group()
    location.add_argument("--install", type=Path, help="Read-only original directory; otherwise discover local/GOG folders")
    location.add_argument("--install-url", help="Default remote install URL for every browser application")
    parser.add_argument("--exporter", type=Path, help="Native cyrus executable for game asset preparation")
    parser.add_argument("--directory", type=Path, default=Path("target/web"))
    parser.add_argument("--page", choices=["index.html", "viewer.html"], default="viewer.html")
    parser.add_argument("--port", type=int, required=True)
    args = parser.parse_args()
    install = locate_install(args.install) if not args.install_url else None
    paths, manifest = original_files(install) if install else ({}, b'{"files":[]}')
    source = None
    if install:
        exporter = args.exporter or shutil.which("cyrus")
        if exporter is None:
            exporter = next((path for path in (Path("target/debug/cyrus"), Path("target/release/cyrus")) if path.is_file()), None)
        exporter = Path(exporter).resolve(strict=True) if exporter else None
        source = GameSource(install, manifest, exporter, Path("cyrus-config/web").resolve())
        listing = json.loads(manifest)
        if exporter:
            listing["cache"] = f"../cyrus-config/cache/{source.generation}/"
            worlds = subprocess.run([str(exporter), "--install", str(install), "--list-worlds"],
                                    capture_output=True, text=True, check=True, timeout=30)
            listing["worlds"] = json.loads(worlds.stdout)
        manifest = json.dumps(listing).encode()
    handler = partial(Handler, directory=str(args.directory), originals=paths, manifest=manifest,
                      source=source, page=args.page, install_url=args.install_url or ("./original/" if install else None))
    print(f"Serving {len(paths)} original files; install selection shared by game and viewer", flush=True)
    Server(("127.0.0.1", args.port), handler).serve_forever()


if __name__ == "__main__":
    main()
