#!/usr/bin/env python3
"""Restore archived Hotaru captures from their recorded Git revision.

The compact reports and executable fixtures remain in the source archive.
Raw recordings live in Git history; current source snapshots omit them.
Requires a Git clone containing the recorded commit. Restored captures are
ignored by Git and verified byte for byte before installation.
"""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
MANIFEST = ROOT / 'apps/hotaru/upstream/evidence.json'


def restore(destination, patterns):
    manifest = json.loads(MANIFEST.read_text())
    revision = manifest['revision']
    if len(revision) != 40 or any(c not in '0123456789abcdef' for c in revision):
        raise ValueError('Invalid evidence revision')
    selected = [entry for entry in manifest['files']
                if not patterns or any(PurePosixPath(entry['path']).match(pattern) for pattern in patterns)]
    if not selected:
        raise ValueError('No captures matched')
    for entry in selected:
        relative = PurePosixPath(entry['path'])
        if relative.is_absolute() or '..' in relative.parts or relative.parts[:3] != ('apps', 'hotaru', 'upstream'):
            raise ValueError(f'Invalid evidence path: {relative}')
        target = destination / relative
        # Never replace an existing capture, nor traverse a directory symlink.
        for parent in [target, *target.parents]:
            if parent.is_symlink():
                raise ValueError(f'Refusing symlink: {parent}')
        if target.exists():
            data = target.read_bytes()
        else:
            data = subprocess.check_output(['git', '-C', str(ROOT), 'show', f'{revision}:{relative}'])
        if len(data) != entry['bytes'] or hashlib.sha256(data).hexdigest() != entry['sha256']:
            raise ValueError(f'Evidence checksum mismatch: {relative}')
        if not target.exists():
            target.parent.mkdir(parents=True, exist_ok=True)
            with tempfile.NamedTemporaryFile(dir=target.parent, delete=False) as output:
                temporary = Path(output.name)
                try:
                    output.write(data)
                    output.flush()
                    # Exclusive installation, including when another restore
                    # races us. A symlink/file appearing here is not replaced.
                    target.hardlink_to(temporary)
                finally:
                    temporary.unlink(missing_ok=True)
        print(relative)
    return len(selected)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('patterns', nargs='*', help='Optional glob(s) matching recorded paths')
    parser.add_argument('--destination', type=Path, default=ROOT,
                        help='Restore below this directory (default: the original checkout paths)')
    args = parser.parse_args()
    restore(args.destination.absolute(), args.patterns)
