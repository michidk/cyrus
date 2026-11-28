#!/usr/bin/env python3
"""Authenticate a large immutable artifact without retaining its archive bytes.

Linux OpenSSL SHA256 checkpoints retain only the portable numeric SHA state.
A checkpoint is not acceptance: only the final expected whole-archive digest is.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import ctypes
import ctypes.util
import json
import os
from pathlib import Path
import urllib.request


class Context(ctypes.Structure):
    # OpenSSL's public SHA256_CTX layout from openssl/sha.h.
    _fields_ = [('h', ctypes.c_uint * 8), ('low', ctypes.c_uint),
                ('high', ctypes.c_uint), ('data', ctypes.c_uint * 16),
                ('num', ctypes.c_uint), ('length', ctypes.c_uint)]


class Digest:
    def __init__(self, saved=None):
        self.lib = ctypes.CDLL(ctypes.util.find_library('crypto') or 'libcrypto.so.3')
        for name in ['SHA256_Init', 'SHA256_Update', 'SHA256_Final']:
            getattr(self.lib, name).restype = ctypes.c_int
        self.lib.SHA256_Init.argtypes = [ctypes.POINTER(Context)]
        self.lib.SHA256_Update.argtypes = [ctypes.POINTER(Context), ctypes.c_void_p, ctypes.c_size_t]
        self.lib.SHA256_Final.argtypes = [ctypes.c_void_p, ctypes.POINTER(Context)]
        self.context = Context()
        if self.lib.SHA256_Init(ctypes.byref(self.context)) != 1:
            raise ValueError('OpenSSL SHA256 initialization failed')
        if saved is not None:
            if set(saved) != {'h', 'low', 'high', 'data', 'num', 'length'}:
                raise ValueError('invalid SHA256 checkpoint fields')
            if len(saved['h']) != 8 or len(saved['data']) != 16:
                raise ValueError('invalid SHA256 checkpoint arrays')
            if any(type(x) is not int or not 0 <= x <= ((1 << 32) - 1)
                   for value in saved.values() for x in (value if isinstance(value, list) else [value])):
                raise ValueError('invalid SHA256 checkpoint word')
            for name, value in saved.items():
                if isinstance(value, list):
                    getattr(self.context, name)[:] = value
                else:
                    setattr(self.context, name, value)
            if self.context.num >= 64 or self.context.length != 32:
                raise ValueError('invalid SHA256 checkpoint buffer')

    def update(self, data):
        if self.lib.SHA256_Update(ctypes.byref(self.context), data, len(data)) != 1:
            raise ValueError('OpenSSL SHA256 update failed')

    def state(self):
        return {name: list(getattr(self.context, name)) if name in {'h', 'data'}
                else getattr(self.context, name) for name, _ in Context._fields_}

    def hexdigest(self):
        copy = Context.from_buffer_copy(self.context)
        result = ctypes.create_string_buffer(32)
        if self.lib.SHA256_Final(result, ctypes.byref(copy)) != 1:
            raise ValueError('OpenSSL SHA256 finalization failed')
        return result.raw.hex()


def atomic_json(path, value):
    stage = path.with_suffix(path.suffix + '.partial')
    with stage.open('w') as stream:
        stream.write(json.dumps(value, indent=2) + '\n'); stream.flush(); os.fsync(stream.fileno())
    stage.replace(path)
    descriptor = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try: os.fsync(descriptor)
    finally: os.close(descriptor)


def authenticate(identity, checkpoint, receipt, read_range, chunk=16 * 1024 * 1024):
    """Resume exact byte ranges; publish acceptance only after whole-file equality."""
    if identity['size'] <= 0 or chunk <= 0 or len(bytes.fromhex(identity['sha256'])) != 32:
        raise ValueError('invalid immutable artifact identity')
    offset, digest = 0, Digest()
    if checkpoint.exists():
        saved = json.loads(checkpoint.read_text())
        if saved['identity'] != identity:
            raise ValueError('checkpoint belongs to another immutable artifact')
        offset = saved['offset']
        digest = Digest(saved['state'])
        bit_count = (digest.context.high << 32) | digest.context.low
        if (type(offset) is not int or not 0 <= offset <= identity['size']
                or bit_count != offset * 8 or digest.context.num != offset % 64):
            raise ValueError('checkpoint byte position differs from SHA state')
    while offset < identity['size']:
        count = min(chunk, identity['size'] - offset)
        data = read_range(offset, count)
        if len(data) != count:
            raise ValueError('artifact returned an incomplete byte range')
        digest.update(data)
        offset += count
        atomic_json(checkpoint, dict(identity=identity, offset=offset, state=digest.state()))
        print(f'authenticated bytes {offset}/{identity["size"]}', flush=True)
    actual = digest.hexdigest()
    if actual != identity['sha256']:
        raise ValueError(f'whole artifact digest mismatch: {actual}')
    atomic_json(receipt, dict(identity=identity, sha256=actual, bytes=offset))
    return actual


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('identity', type=Path, help='immutable API identity JSON: artifact/run/size/sha256')
    p.add_argument('url_file', type=Path, help='private signed download URL; never logged')
    p.add_argument('checkpoint', type=Path)
    p.add_argument('receipt', type=Path)
    args = p.parse_args()
    identity = json.loads(args.identity.read_text())
    url = args.url_file.read_text().strip()
    def read_range(offset, count):
        request = urllib.request.Request(url, headers={'Range': f'bytes={offset}-{offset + count - 1}'})
        with urllib.request.urlopen(request, timeout=120) as response:
            expected = f'bytes {offset}-{offset + count - 1}/{identity["size"]}'
            if response.status != 206 or response.headers.get('Content-Range') != expected:
                raise ValueError('server did not return the exact requested artifact range')
            return response.read(count + 1)
    chunk = 16 * 1024 * 1024
    pending = {}
    with ThreadPoolExecutor(max_workers=4) as executor:
        def prefetched(offset, count):
            for position in range(offset, min(identity['size'], offset + 4 * chunk), chunk):
                if position not in pending:
                    pending[position] = executor.submit(read_range, position, min(chunk, identity['size'] - position))
            return pending.pop(offset).result()
        authenticate(identity, args.checkpoint, args.receipt, prefetched, chunk)


if __name__ == '__main__':
    main()
