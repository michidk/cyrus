#!/usr/bin/env python3
"""Prepare smaller unpack transactions from a retained Docker runtime archive.

Runtime configuration and filesystem entries are preserved. Large layers may
contain only ordinary files, directories and symbolic links: whiteouts, hardlinks
and special entries reject rather than changing their cross-layer semantics.
A single file stays indivisible and may exceed the requested layer budget.
Host receipts reuse completed source layers and the final archive after restart;
Docker's unpacked snapshots still belong to its current daemon.
"""
import argparse
import copy
import gzip
import hashlib
import io
import json
from pathlib import Path
import tarfile

from harness import exclusive_lock
from native_session_capture import atomic, file_digest


class HashedWriter:
    def __init__(self, stream):
        self.stream, self.digest = stream, hashlib.sha256()

    def write(self, data):
        self.digest.update(data)
        return self.stream.write(data)


def json_write(path, value):
    atomic(path, (json.dumps(value, sort_keys=True) + '\n').encode())


def reusable(path, binding):
    try:
        receipt = json.loads(path.read_text())
        return (receipt if receipt['binding'] == binding and all(
            file_digest(path.parent / item['path']) == item['sha256']
            for item in receipt['files']) else None)
    except (OSError, ValueError, KeyError, TypeError):
        return None


def allowed(member):
    path = Path(member.name)
    if (path.is_absolute() or '..' in path.parts or path.name.startswith('.wh.')
            or member.type not in (tarfile.REGTYPE, tarfile.AREGTYPE, tarfile.DIRTYPE, tarfile.SYMTYPE)
            or any('overlay' in key for key in member.pax_headers)):
        raise ValueError(f'unsupported layer entry: {member.name}')


def chunk_layer(archive, name, directory, prefix, budget):
    """Preserve entry order, including directory metadata and executable modes."""
    results, stack, size = [], None, 0
    try:
        with tarfile.open(fileobj=archive.extractfile(name), mode='r|*') as layer:
            for member in layer:
                allowed(member)
                if stack is not None and size and size + member.size > budget:
                    results.append(close_chunk(stack, directory))
                    stack, size = None, 0
                if stack is None:
                    stack = open_chunk(directory / f'{prefix}-{len(results):04}.tar.gz')
                stack[-1].addfile(copy.copy(member), layer.extractfile(member) if member.isfile() else None)
                size += member.size
        if stack is not None:
            results.append(close_chunk(stack, directory))
            stack = None
    finally:
        if stack is not None:
            close_chunk(stack, directory)
    return results


def open_chunk(path):
    raw = path.with_suffix(path.suffix + '.partial').open('wb')
    compressed = gzip.GzipFile(fileobj=raw, mode='wb', compresslevel=1, mtime=0)
    hashed = HashedWriter(compressed)
    tar = tarfile.open(fileobj=hashed, mode='w|', format=tarfile.PAX_FORMAT)
    return path, raw, compressed, hashed, tar


def close_chunk(stack, directory):
    path, raw, compressed, hashed, tar = stack
    tar.close()
    compressed.close()
    raw.flush()
    import os
    os.fsync(raw.fileno())
    raw.close()
    path.with_suffix(path.suffix + '.partial').replace(path)
    return dict(path=str(path.relative_to(directory)), sha256=file_digest(path),
                diff_id='sha256:' + hashed.digest.hexdigest())


def source_layer(archive, name, directory, index, budget, binding, original_diff):
    receipt = directory / f'layer-{index:04}.json'
    key = dict(binding, layer=name, index=index, original_diff=original_diff)
    previous = reusable(receipt, key)
    if previous is not None:
        return previous['files']
    if archive.getmember(name).size > budget:
        files = chunk_layer(archive, name, directory, f'layer-{index:04}', budget)
    else:
        path = directory / f'layer-{index:04}.original'
        temporary = path.with_suffix('.partial')
        with archive.extractfile(name) as source, temporary.open('wb') as output:
            import shutil
            shutil.copyfileobj(source, output)
        temporary.replace(path)
        files = [dict(path=path.name, sha256=file_digest(path), diff_id=original_diff)]
    json_write(receipt, dict(binding=key, files=files))
    return files


def add_bytes(archive, name, data):
    member = tarfile.TarInfo(name)
    member.size = len(data)
    archive.addfile(member, io.BytesIO(data))


def publish(output, image, config, layers, directory):
    config = copy.deepcopy(config)
    config['rootfs']['diff_ids'] = [layer['diff_id'] for layer in layers]
    # History is informational; source commands no longer correspond one-to-one
    # to unpack transactions. Preserve the executable environment/config intact.
    config['history'] = [{'created_by': 'cyrus retained runtime layer preparation'} for _ in layers]
    data = json.dumps(config, sort_keys=True).encode()
    name = hashlib.sha256(data).hexdigest() + '.json'
    manifest = [dict(Config=name, RepoTags=[image], Layers=[layer['path'] for layer in layers])]
    temporary = output.with_suffix(output.suffix + '.partial')
    with tarfile.open(temporary, 'w') as archive:
        add_bytes(archive, name, data)
        add_bytes(archive, 'manifest.json', json.dumps(manifest).encode())
        for layer in layers:
            archive.add(directory / layer['path'], arcname=layer['path'], recursive=False)
    temporary.replace(output)


def prepare(source, output, image, budget):
    if source.resolve() == output.resolve() or budget <= 0:
        raise ValueError('source and output must differ and layer budget must be positive')
    output.parent.mkdir(parents=True, exist_ok=True)
    directory = Path(str(output) + '.layers')
    directory.mkdir(exist_ok=True)
    binding = dict(source_sha256=file_digest(source), image=image, layer_bytes=budget,
                   tool_sha256=file_digest(Path(__file__)))
    identity = hashlib.sha256(json.dumps(binding, sort_keys=True).encode()).hexdigest()[:12]
    tag = image + '-layered-' + str(budget) + '-' + identity
    with exclusive_lock(Path(str(output) + '.lock')):
        receipt = Path(str(output) + '.json')
        if reusable(receipt, binding) is not None:
            return tag
        with tarfile.open(source) as archive:
            manifests = json.load(archive.extractfile('manifest.json'))
            matches = [entry for entry in manifests if image in entry.get('RepoTags', [])]
            if len(matches) != 1:
                raise ValueError(f'{source}: expected one manifest for {image}')
            manifest = matches[0]
            config = json.load(archive.extractfile(manifest['Config']))
            diffs = config['rootfs']['diff_ids']
            if len(diffs) != len(manifest['Layers']):
                raise ValueError(f'{source}: layer/config count differs')
            layers = []
            for index, (name, diff) in enumerate(zip(manifest['Layers'], diffs)):
                layers.extend(source_layer(archive, name, directory, index, budget, binding, diff))
        publish(output, tag, config, layers, directory)
        json_write(receipt, dict(binding=binding, files=[dict(path=output.name, sha256=file_digest(output))],
                                 image=tag, layers=layers))
    return tag


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('image')
    parser.add_argument('--layer-bytes', type=int, default=64 * 1024 * 1024)
    args = parser.parse_args()
    try:
        print(prepare(args.source, args.output, args.image, args.layer_bytes))
    except (OSError, ValueError, KeyError, tarfile.TarError) as error:
        parser.exit(1, f'runtime layer preparation: {error}\n')


if __name__ == '__main__':
    main()
