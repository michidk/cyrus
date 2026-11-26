#!/usr/bin/env python3
"""Project a headless observer runtime from a retained original-runner archive.

Resolve ELF dependencies from the retained filesystem, never the host linker.
The separate image omits browser serving and uses the existing SDL surface and
software Voodoo configuration. It is not a replacement for the browser rig.
"""
import argparse
import copy
from contextlib import ExitStack
import hashlib
import gzip
import shutil
import importlib.util
import json
import os
from pathlib import Path, PurePosixPath
import posixpath
import re
import subprocess
import tarfile
import tempfile

from harness import exclusive_lock
from native_session_capture import file_digest

spec = importlib.util.spec_from_file_location('runtime_layers', Path(__file__).with_name('layer-runtime-archive.py'))
layers = importlib.util.module_from_spec(spec)
spec.loader.exec_module(layers)

PROGRAMS = ('seq', 'true', 'bash', 'dash', 'sh', 'env', 'sleep', 'find', 'cp', 'cat', 'mkdir', 'rm',
            'Xvfb', 'xdpyinfo', 'xdotool', 'xkbcomp', 'scrot', 'dosbox-x', 'python3')
DATA = ('usr/lib/python3.14', 'usr/lib/python3/dist-packages/PIL',
        'usr/share/X11/xkb', 'usr/share/fonts/X11', 'etc/fonts',
        'usr/share/fontconfig', 'usr/share/dosbox-x', 'etc/python3.14',
        'usr/lib/x86_64-linux-gnu/imlib2/loaders/png.so')
ENTRYPOINT = b'''#!/bin/bash
set -euo pipefail
export DISPLAY=:1 SDL_AUDIODRIVER=dummy
Xvfb :1 -screen 0 "${SCREEN:-1024x768x24}" -nolisten tcp &
xpid=$!
trap 'kill "$xpid" 2>/dev/null || true' EXIT
for ((i=0; i<100; i++)); do
    if xdpyinfo >/dev/null 2>&1; then break; fi
    sleep .05
done
xdpyinfo >/dev/null
cd /game
[ -s game.cue ] || cp game.ins game.cue
dosbox-x -conf /etc/cyrus/dosbox-x.conf "$@"
'''


def name(value):
    path = PurePosixPath(value)
    if path.is_absolute() or '..' in path.parts or not path.parts:
        raise ValueError(f'unsafe runtime path: {value}')
    return str(path)


def elf_dependencies(data):
    """Read ELF metadata without executing any retained or host executable."""
    if not data.startswith(b'\x7fELF'):
        return []
    with tempfile.NamedTemporaryFile() as source:
        source.write(data)
        source.flush()
        result = subprocess.run(['readelf', '-W', '-l', '-d', source.name],
                                text=True, capture_output=True, check=True)
    needed = re.findall(r'\(NEEDED\).*?\[(.*?)\]', result.stdout)
    interpreter = re.findall(r'Requesting program interpreter: (.*?)\]', result.stdout)
    return needed + interpreter


def cached_layer(archive, layer_name, directory):
    """Keep completed decompression outside the disposable Docker daemon."""
    key = hashlib.sha256(layer_name.encode()).hexdigest()
    path = directory / (key + '.tar')
    receipt = directory / (key + '.json')
    binding = dict(source_layer_sha256=file_digest_stream(archive.extractfile(layer_name)))
    if layers.reusable(receipt, binding) is None:
        temporary = path.with_suffix('.partial')
        with archive.extractfile(layer_name) as source, temporary.open('wb') as output:
            header = source.read(2)
            source.seek(0)
            if header == b'\x1f\x8b':
                with gzip.GzipFile(fileobj=source) as decoded:
                    shutil.copyfileobj(decoded, output)
            else:
                shutil.copyfileobj(source, output)
            output.flush()
            os.fsync(output.fileno())
        temporary.replace(path)
        layers.json_write(receipt, dict(binding=binding,
            files=[dict(path=path.name, sha256=file_digest(path))]))
    return path


def file_digest_stream(stream):
    with stream:
        digest = hashlib.sha256()
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(block)
        return digest.hexdigest()


def project(archive, manifest, stack, roots, directory):
    entries = {}
    for layer_name in manifest['Layers']:
        layer = stack.enter_context(tarfile.open(cached_layer(archive, layer_name, directory)))
        for member in layer:
            key = name(member.name)
            # The supported retained image has no deletions. Reject rather than
            # projecting a stale dependency across an unhandled whiteout.
            if PurePosixPath(key).name.startswith('.wh.'):
                raise ValueError(f'whiteout requires filesystem merge: {key}')
            entries[key] = (layer, member)
    selected, visiting = {}, set()

    def add(key):
        key = name(key)
        if key in selected:
            return
        if key in visiting:
            raise ValueError(f'cyclic runtime link: {key}')
        if key not in entries:
            # Resolve intermediate directory links, retaining them in the image.
            parts = PurePosixPath(key).parts
            for count in range(1, len(parts)):
                parent = '/'.join(parts[:count])
                if parent in entries and entries[parent][1].issym():
                    add(parent)
                    target = link_target(parent, entries[parent][1].linkname)
                    add(target + '/' + '/'.join(parts[count:]))
                    return
            raise ValueError(f'missing retained runtime path: {key}')
        visiting.add(key)
        layer, member = entries[key]
        if member.issym() or member.islnk():
            target = (link_target(key, member.linkname) if member.issym() else name(member.linkname))
            add(target)
        elif member.isfile():
            selected[key] = (layer, member)
            for dependency in elf_dependencies(layer.extractfile(member).read()):
                if dependency.startswith('/'):
                    add(dependency.lstrip('/'))
                else:
                    candidates = ['usr/lib/x86_64-linux-gnu/' + dependency,
                                  'usr/lib/' + dependency]
                    match = next((value for value in candidates if value in entries), None)
                    if match is None:
                        nested = [value for value in entries if value.startswith('usr/lib/')
                                  and PurePosixPath(value).name == dependency]
                        if len(nested) == 1:
                            match = nested[0]
                    if match is None:
                        raise ValueError(f'{key}: missing retained ELF dependency {dependency}')
                    add(match)
        elif not member.isdir():
            raise ValueError(f'unsupported runtime entry: {key}')
        selected[key] = (layer, member)
        visiting.remove(key)
        # Preserve directory aliases such as /bin -> usr/bin.
        for parent in PurePosixPath(key).parents:
            if str(parent) != '.' and str(parent) in entries:
                parent_member = entries[str(parent)][1]
                if parent_member.isdir():
                    selected[str(parent)] = entries[str(parent)]

    for root in roots:
        add(root)
        subtree = root
        while subtree in entries and entries[subtree][1].issym():
            subtree = link_target(subtree, entries[subtree][1].linkname)
        if subtree in entries and entries[subtree][1].isdir():
            for key in entries:
                if key.startswith(subtree + '/') and '__pycache__' not in PurePosixPath(key).parts:
                    add(key)
    return selected


def link_target(key, target):
    value = posixpath.normpath(target.lstrip('/') if target.startswith('/') else
                              posixpath.join(posixpath.dirname(key), target))
    return name(value)


def prepare(source, output, image, roots):
    if source.resolve() == output.resolve():
        raise ValueError('source and output must differ')
    binding = dict(source_sha256=file_digest(source), tool_sha256=file_digest(Path(__file__)),
                   layer_tool_sha256=file_digest(Path(layers.__file__)), roots=roots, image=image)
    tag = image + '-headless-' + hashlib.sha256(json.dumps(binding, sort_keys=True).encode()).hexdigest()[:12]
    output.parent.mkdir(parents=True, exist_ok=True)
    receipt = Path(str(output) + '.json')
    with exclusive_lock(Path(str(output) + '.lock')):
        if layers.reusable(receipt, binding) is not None:
            return tag
        directory = Path(str(output) + '.layers')
        directory.mkdir(exist_ok=True)
        with ExitStack() as stack:
            archive = stack.enter_context(tarfile.open(source))
            manifests = json.load(archive.extractfile('manifest.json'))
            matches = [item for item in manifests if image in item.get('RepoTags', [])]
            if len(matches) != 1:
                raise ValueError('expected exactly one retained source image')
            manifest = matches[0]
            selected = project(archive, manifest, stack, roots, directory)
            payload = directory / 'headless.tar'
            temporary = payload.with_suffix('.partial')
            with tarfile.open(temporary, 'w') as target:
                for key in sorted(selected):
                    layer, member = selected[key]
                    original = member
                    if member.islnk():
                        source_layer, original = selected[name(member.linkname)]
                        while original.islnk():
                            source_layer, original = selected[name(original.linkname)]
                        if not original.isfile():
                            raise ValueError(f'{key}: hardlink target is not a regular runtime file')
                    else:
                        source_layer = layer
                    stream = source_layer.extractfile(original) if original.isfile() else None
                    member = copy.copy(member)
                    if member.islnk():
                        # Multicall executables check their actual executable name.
                        # Materialize bytes instead of changing hardlinks to symlinks.
                        member.type, member.size = tarfile.REGTYPE, original.size
                        member.linkname = ''
                    target.addfile(member, stream)
                for runtime_dir, mode in [('tmp', 0o1777), ('run', 0o755)]:
                    member = tarfile.TarInfo(runtime_dir)
                    member.type, member.mode = tarfile.DIRTYPE, mode
                    target.addfile(member)
                entry = tarfile.TarInfo('usr/local/bin/headless-retro')
                entry.mode, entry.size = 0o755, len(ENTRYPOINT)
                import io
                target.addfile(entry, io.BytesIO(ENTRYPOINT))
            temporary.replace(payload)
            os.utime(payload, (0, 0))
            digest = file_digest(payload)
            chunks = [dict(path=payload.name, sha256=digest, diff_id='sha256:' + digest)]
            config = json.load(archive.extractfile(manifest['Config']))
            config['config']['Entrypoint'] = ['/usr/local/bin/headless-retro']
            config['config']['Cmd'] = None
            layers.publish(output, tag, config, chunks, directory)
        layers.json_write(receipt, dict(binding=binding, image=tag, selected=list(sorted(selected)),
            files=[dict(path=output.name, sha256=file_digest(output))]))
    return tag


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('image')
    parser.add_argument('--root', action='append', help='exact retained path or subtree; repeatable')
    args = parser.parse_args()
    roots = args.root or ([f'usr/bin/{program}' for program in PROGRAMS] + list(DATA) +
                          ['bin/bash', 'etc/cyrus/dosbox-x.conf'])
    try:
        print(prepare(args.archive, args.output, args.image, roots))
    except (OSError, ValueError, KeyError, tarfile.TarError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'headless runtime preparation: {error}\n')


if __name__ == '__main__':
    main()
