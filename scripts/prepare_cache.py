#!/usr/bin/env python3
"""Stage signed NARs only for runtime paths not already served by a cache.

The publication wrapper uploads all archives before their narinfos. Upstream
references need not be copied into Sekka's cache: clients resolve each reference
from their configured substituters, and the separate coverage gate checks them.
"""
import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
from functools import partial
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

from check_cache_coverage import CACHES, archive_error, cache_info, invalid_paths


DESTINATION = 'https://cache.sekka.computer'
# The other caches Sekka systems use, as the coverage gate lists them: what
# they serve is not published again.
UPSTREAMS = [cache for cache in CACHES if cache != DESTINATION]


def command(*args, input=None):
    return subprocess.run(args, input=input, text=True, capture_output=True, check=True).stdout


def missing_from(cache, paths):
    """Batch existence checks without downloading or recompressing store paths."""
    paths = sorted(paths)
    batches = [paths[i:i + 128] for i in range(0, len(paths), 128)]
    missing = set()
    with ThreadPoolExecutor(max_workers=4) as pool:
        for invalid in pool.map(partial(invalid_paths, cache), batches):
            missing.update(invalid)
    return missing


def unpublished(info, upstreams, destination):
    missing = set(info)
    for cache in upstreams:
        if missing:
            missing = missing_from(cache, missing)
    excluded = {path for path in missing if '-determinate-nixd-' in Path(path).name}
    if excluded:
        # Preserve the existing publication policy. This output is assembled
        # locally from Determinate's download, not necessarily in its cache.
        # The separate coverage gate verifies that exact fallback needs no
        # compiler, including any affected cached ancestors.
        print('Leaving Determinate Nixd to its upstream download: '
              + ', '.join(sorted(excluded)), file=sys.stderr)
        missing -= excluded
    # Unlike the old upload-time deduplication, an existing Sekka narinfo avoids
    # compression too. Do not skip a dangling/truncated archive: repair it.
    existing = cache_info(destination, missing)
    candidates = {path: data for path, data in existing.items() if data is not None}
    with ThreadPoolExecutor(max_workers=8) as pool:
        for path, error in zip(candidates, pool.map(
                lambda path: archive_error(destination, candidates[path]), candidates)):
            if error is not None:
                print(f'Repairing unavailable archive for {path}: {error}', file=sys.stderr)
                continue
            cached = candidates[path]
            local = info[path]
            if cached['narHash'] != local['narHash'] or cached['narSize'] != local['narSize'] \
                    or set(cached['references']) != set(local['references']):
                # The same derivation built differently, as one that is not
                # reproducible is on another machine. The published copy is
                # valid and signed; replacing it would only alternate the
                # cache between the two with every publishing machine.
                print(f'Keeping the published copy of {path}, which differs from the local build',
                      file=sys.stderr)
            missing.remove(path)
    return sorted(missing)


def sri_sha256(value):
    if not value.startswith('sha256-'):
        raise ValueError(f'Expected a SHA-256 SRI hash, got {value}')
    digest = base64.b64decode(value[len('sha256-'):], validate=True)
    if len(digest) != 32:
        raise ValueError('Invalid SHA-256 digest length')
    return digest


def export_path(path, info, stage):
    """Stream one canonical NAR into zstd; never export its cached references."""
    temporary = tempfile.NamedTemporaryFile(prefix='.nar-', dir=stage, delete=False)
    archive = Path(temporary.name)
    digest, size = hashlib.sha256(), 0
    try:
        with temporary, tempfile.TemporaryFile() as dump_error, tempfile.TemporaryFile() as zip_error:
            # Leaving the inner block closes this end of each pipe and waits:
            # an exporter still writing fails, and zstd sees its input end.
            with subprocess.Popen(['nix', 'store', 'dump-path', path],
                                  stdout=subprocess.PIPE, stderr=dump_error) as dump, \
                    subprocess.Popen(['zstd', '-q', '-T2', '-3', '-c'], stdin=subprocess.PIPE,
                                     stdout=temporary, stderr=zip_error) as compress:
                try:
                    while block := dump.stdout.read(1024 * 1024):
                        digest.update(block)
                        size += len(block)
                        compress.stdin.write(block)
                except BrokenPipeError:
                    pass  # zstd stopped early; its status and messages follow
                finally:
                    try:
                        compress.stdin.close()
                    except BrokenPipeError:
                        pass
            if dump.returncode or compress.returncode:
                messages = []
                for stream in (dump_error, zip_error):
                    stream.seek(0)
                    messages.append(stream.read().decode(errors='replace'))
                raise ValueError(f'NAR export failed for {path}: ' + ''.join(messages))
        if digest.digest() != sri_sha256(info['narHash']) or size != info['narSize']:
            raise ValueError(f'Exported NAR does not match registered contents: {path}')
        with archive.open('rb') as stream:
            file_hash = hashlib.file_digest(stream, 'sha256').hexdigest()
        url = f'nar/{file_hash}.nar.zst'
        file_size = archive.stat().st_size
        archive.rename(stage / url)
        fields = [
            f'StorePath: {path}', f'URL: {url}', 'Compression: zstd',
            f'FileHash: sha256:{file_hash}', f'FileSize: {file_size}',
            f'NarHash: sha256:{digest.hexdigest()}', f'NarSize: {size}',
            'References: ' + ' '.join(Path(ref).name for ref in sorted(info['references'])),
        ]
        if info.get('deriver'):
            fields.append('Deriver: ' + Path(info['deriver']).name)
        if info.get('ca'):
            # As nix copy writes it: a client may accept a content-addressed
            # path by its contents alone, without a signature.
            fields.append('CA: ' + info['ca'])
        # Nix canonicalizes the metadata and creates its own signature below.
        (stage / (Path(path).name.split('-', 1)[0] + '.narinfo')).write_text('\n'.join(fields) + '\n')
    finally:
        archive.unlink(missing_ok=True)


def prepare(outputs, stage, key, upstreams, destination, jobs=4):
    stage = Path(stage).resolve()
    stage.mkdir(parents=True, exist_ok=True)
    if any(stage.iterdir()):
        raise ValueError('The staging directory must be empty')
    info = json.loads(command('nix', 'path-info', '--recursive', '--json', '--json-format', '1', *outputs))
    if not info or any(value is None for value in info.values()):
        raise ValueError('The publication roots must exist in the local store')
    paths = unpublished(info, upstreams, destination)
    (stage / 'nar').mkdir()
    # A failed export cancels the exports not started yet.
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        list(pool.map(lambda path: export_path(path, info[path], stage), paths))
    if paths:
        # Nix completes the stage as a binary cache (nix-cache-info) as it signs.
        command('nix', 'store', 'sign', '--store', stage.as_uri(), '--key-file', str(key),
                '--stdin', input='\n'.join(paths) + '\n')
    print(f'Staged {len(paths)} of {len(info)} paths; others are cached or excluded by policy', flush=True)
    return paths


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--stage', required=True)
    parser.add_argument('--key-file', required=True)
    parser.add_argument('--upstream', action='append', default=None)
    parser.add_argument('--destination', default=DESTINATION)
    parser.add_argument('--jobs', type=int, default=4)
    parser.add_argument('outputs', nargs='+')
    args = parser.parse_args(argv)
    if args.jobs < 1:
        parser.error('--jobs must be positive')
    prepare(args.outputs, args.stage, args.key_file,
            UPSTREAMS if args.upstream is None else args.upstream, args.destination, args.jobs)


if __name__ == '__main__':
    try:
        main(sys.argv[1:])
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(f'Cache preparation failed: {error}', file=sys.stderr)
        if isinstance(error, subprocess.CalledProcessError) and error.stderr:
            print(error.stderr, file=sys.stderr)
        sys.exit(1)
