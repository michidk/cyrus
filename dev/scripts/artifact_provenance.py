#!/usr/bin/env python3
"""Publish checkout identity beside packages in the same immutable CI artifact."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess


def write_provenance(directory, head, environment=os.environ):
    """Bind standard packages to the actual Git checkout and workflow attempt."""
    def git(*args):
        return subprocess.check_output(['git', *args], text=True).strip()
    subprocess.run(['git', 'diff', '--quiet', 'HEAD'], check=True)
    packages = {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                for path in sorted(directory.glob('cyrus-*.tar.gz'))}
    packages.update({path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                     for path in sorted(directory.glob('cyrus-*.zip'))})
    if not packages:
        raise ValueError('artifact provenance requires packaged executables')
    record = dict(version=1, repository=environment['GITHUB_REPOSITORY'],
                  run=int(environment['GITHUB_RUN_ID']),
                  attempt=int(environment['GITHUB_RUN_ATTEMPT']), head=head,
                  checkout_commit=git('rev-parse', 'HEAD'),
                  checkout_tree=git('rev-parse', 'HEAD^{tree}'), packages=packages)
    (directory / 'source-provenance.json').write_text(json.dumps(record, indent=2) + '\n')
    return record


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    parser.add_argument('head')
    args = parser.parse_args()
    write_provenance(args.directory, args.head)
