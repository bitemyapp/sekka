#!/usr/bin/env python3
"""What a Sekka system with an empty store would build rather than download.

Usage: scripts/check_cache_coverage.py [--allow NAME]... [--cache URL]... INSTALLABLE...

Starting from each installable's derivation, follow what none of the caches
Sekka systems use has, the way Nix would: a derivation whose wanted outputs
are all in a cache is downloaded; one that isn't is built, which needs its
inputs. The derivations that would be built are listed in four kinds:

- local: meant to be built where they are used (preferLocalBuild, or
  allowSubstitutes off), such as configuration files: quick by design;
- downloads: fixed-output derivations, which fetch their sources;
- copies: built without a C compiler (stdenvNoCC) or any other compiler or
  build tool among their inputs, such as Determinate Nixd put in place from
  its download;
- compiles: everything else.

Cached outputs' runtime references are checked across these caches, including
the archives named by their metadata. Missing metadata can require rebuilding
the affected cached ancestors too; those exact derivations use the same build
classification. Archive probes check existence and advertised length, not the
full contents; Nix verifies downloads when used.

It exits with 1 when runtime coverage is incomplete or anything would be
compiled, other than derivations named with --allow (by name without the hash
and `.drv`). The nightly
workflow runs it before moving `nightly`, so updates never compile.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import json
from pathlib import Path
import subprocess
import sys
from urllib.error import HTTPError, URLError
from urllib.parse import unquote, urljoin, urlsplit
from urllib.request import Request, urlopen

CACHES = [
    'https://cache.nixos.org',
    'https://cache.sekka.computer',
    'https://cache.numtide.com',
    'https://install.determinate.systems',
]
STORE = '/nix/store/'


def nix(*args):
    return subprocess.run(['nix', *args], check=True, capture_output=True, text=True).stdout


def show(drvs):
    """Each derivation's outputs, inputs and settings."""
    found = {}
    drvs = sorted(drvs)
    for i in range(0, len(drvs), 200):
        value = json.loads(nix('derivation', 'show', *drvs[i:i + 200]))
        for name, info in value.get('derivations', value).items():
            found[name if name.startswith(STORE) else STORE + name] = info
    return found


def outputs(drv, info):
    """Output name to store path; a fixed-output derivation's paths come from the store."""
    paths = {name: STORE + out['path'].removeprefix(STORE) for name, out in info['outputs'].items() if 'path' in out}
    if len(paths) < len(info['outputs']):
        listed = subprocess.run(['nix-store', '-q', '--outputs', drv], check=True, capture_output=True,
                                text=True).stdout.split()
        for path in listed:
            name = next((n for n in info['outputs'] if n != 'out' and path.endswith('-' + n)), 'out')
            paths.setdefault(name, path)
    return paths


def cache_info(cache, paths):
    """Read fresh metadata through Nix without substituting any store paths."""
    # path-info can fail before emitting JSON for a missing path. Filter misses
    # first, and fail closed on network errors or malformed metadata.
    options = ['--option', 'narinfo-cache-positive-ttl', '0',
               '--option', 'narinfo-cache-negative-ttl', '0',
               '--option', 'http-connections', '4']
    def batch_info(batch):
        result = subprocess.run(
            ['nix-store', '--store', cache, *options, '--check-validity', '--print-invalid', *batch],
            check=True, capture_output=True, text=True, timeout=180)
        missing = set(result.stdout.split())
        if not missing <= set(batch):
            raise ValueError(f'{cache}: unexpected missing paths')
        present = sorted(set(batch) - missing)
        info = {}
        if present:
            result = subprocess.run(
                ['nix', 'path-info', '--store', cache, *options, '--json', '--json-format', '1', '--stdin'],
                input='\n'.join(present) + '\n', check=True, capture_output=True, text=True, timeout=180)
            info = json.loads(result.stdout)
            if set(info) != set(present) or any(value is None for value in info.values()):
                raise ValueError(f'{cache}: unexpected store path metadata')
        return dict.fromkeys(missing) | info

    paths = sorted(paths)
    batches = [paths[i:i + 128] for i in range(0, len(paths), 128)]
    found = {}
    with ThreadPoolExecutor(max_workers=4) as pool:
        for batch in pool.map(batch_info, batches):
            found.update(batch)
    return found


def archive_error(cache, info):
    """A bounded archive availability probe, without downloading a whole NAR."""
    base = urlsplit(cache)._replace(query='', fragment='').geturl()
    url = urljoin(base.rstrip('/') + '/', info['url'])
    expected = info.get('downloadSize')
    try:
        parsed = urlsplit(url)
        if parsed.scheme == 'file':
            with Path(unquote(parsed.path)).open('rb') as stream:
                size = stream.seek(0, 2)
        elif parsed.scheme in ('https', 'http'):
            # Some binary caches reject generic Python clients. Identify this
            # Nix cache verifier explicitly, without pretending to be a browser.
            headers = {'User-Agent': 'Nix/sekka-cache-coverage'}
            try:
                response = urlopen(Request(url, method='HEAD', headers=headers), timeout=30)
            except HTTPError as error:
                error.close()
                if error.code not in (405, 501):
                    raise
                response = urlopen(Request(url, headers=headers | {'Range': 'bytes=0-0'}), timeout=30)
            with response:
                if response.status == 206:
                    size = int(response.headers['Content-Range'].rsplit('/', 1)[1])
                elif response.status == 200:
                    size = response.headers.get('Content-Length')
                    size = int(size) if size is not None else None
                else:
                    raise ValueError(f'unexpected HTTP status {response.status}')
        else:
            raise ValueError(f'unsupported archive URL scheme {parsed.scheme}')
        if expected is not None and size != expected:
            raise ValueError(f'archive length {size}, expected {expected}')
    except (OSError, URLError, ValueError, KeyError) as error:
        if isinstance(error, HTTPError):
            error.close()
        return f'{url}: {error}'
    return None


class CacheCoverage:
    def __init__(self, caches):
        self.caches = caches
        self.metadata = {}
        self.archives = {}
        self.cached_paths = set()
        self.references = {}
        self.required_by = {}
        self.missing_runtime = set()

    def find(self, paths, archives=False):
        remaining, found, errors = set(paths), {}, {}
        for cache in self.caches:
            if not remaining:
                break
            known = self.metadata.setdefault(cache, {})
            known.update(cache_info(cache, remaining - known.keys()))
            available = {path: known[path] for path in remaining if known[path] is not None}
            if archives:
                unchecked = [path for path in available if (cache, path) not in self.archives]
                with ThreadPoolExecutor(max_workers=8) as pool:
                    for path, error in zip(unchecked, pool.map(
                            lambda path: archive_error(cache, available[path]), unchecked)):
                        self.archives[cache, path] = error
            for path, info in available.items():
                error = self.archives[cache, path] if archives else None
                if error:
                    errors.setdefault(path, []).append(error)
                else:
                    found[path] = info
                    remaining.remove(path)
        self.cached_paths.update(found)
        return found, remaining, errors

    def uncached(self, paths):
        return self.find(paths)[1]

    def runtime_fallbacks(self):
        """Walk References and find outputs Nix cannot directly substitute."""
        seen = set(self.references) | self.missing_runtime
        pending = {path: 'requested output' for path in self.cached_paths - seen}
        while pending:
            print(f'runtime: checking {len(pending)} paths ({len(seen)} checked)', flush=True)
            found, missing, errors = self.find(pending, archives=True)
            broken = missing & errors.keys()
            if broken:
                details = [f'{path} (required by {pending[path]}): '
                           + '; '.join(errors[path]) for path in sorted(broken)]
                raise ValueError('Incomplete runtime cache coverage:\n' + '\n'.join(details))
            self.required_by.update(pending)
            self.missing_runtime.update(missing)
            self.references.update({path: info['references'] for path, info in found.items()})
            seen.update(pending)
            pending = {reference: path for path, info in found.items()
                       for reference in info['references'] if reference not in seen}
        # Nix falls back to building the cached parent when a reference cannot
        # be substituted. Classify ancestors too: a local leaf alone does not
        # prove its cached compiler-built parent is safe to rebuild.
        affected = set(self.missing_runtime)
        while True:
            parents = {path for path, refs in self.references.items() if affected.intersection(refs)}
            if parents <= affected:
                return affected
            affected.update(parents)


def settings(info):
    """A derivation's settings, wherever its format keeps them."""
    env = dict(info.get('env', {}))
    if '__json' in env:
        env.update(json.loads(env['__json']))
    env.update(info.get('structuredAttrs') or {})
    return env


# Inputs that mean a derivation compiles (by name, without the hash).
COMPILERS = ('rustc-', 'cargo-', 'go-', 'gcc-', 'clang-', 'llvm-', 'cmake-', 'meson-', 'ninja-', 'ghc-',
             'zig-', 'openjdk-', 'jdk-', 'nodejs-', 'stdenv-linux.drv')


def kind(info):
    env = settings(info)
    if any('hash' in out for out in info['outputs'].values()):
        return 'downloads'
    local = env.get('preferLocalBuild') in ('1', True)
    no_substitutes = 'allowSubstitutes' in env and env['allowSubstitutes'] in ('', '0', False)
    if local or no_substitutes:
        return 'local'
    inputs = [name.removeprefix(STORE)[33:] for name in info['inputs']['drvs']]
    if any(name.startswith('stdenv-linux-no-cc') for name in inputs) and not any(
            name.startswith(COMPILERS) for name in inputs):
        return 'copies'
    return 'compiles'


def builds_for(wanted, coverage, force=()):
    """Classify missing outputs and the inputs needed to build them."""
    wanted = dict(wanted)  # None: every output
    processed, built = {}, {}
    pending = set(wanted)
    while pending:
        infos = show(pending)
        paths = {drv: outputs(drv, info) for drv, info in infos.items()}
        requested = {drv: (set(outs) if wanted[drv] is None else wanted[drv])
                     - processed.get(drv, set()) for drv, outs in paths.items()}
        for drv, names in requested.items():
            processed.setdefault(drv, set()).update(names)
        needed = [path for drv, outs in paths.items() for name, path in outs.items()
                  if name in requested[drv]]
        missing = coverage.uncached(needed) | set(force)
        pending = set()
        for drv, info in infos.items():
            if not any(path in missing for name, path in paths[drv].items()
                       if name in requested[drv]):
                continue
            built[drv] = info
            for dep, want in info['inputs']['drvs'].items():
                dep = dep if dep.startswith(STORE) else STORE + dep
                outs = set(want.get('outputs', want) if isinstance(want, dict) else want)
                if dep not in wanted:
                    wanted[dep] = outs
                elif wanted[dep] is not None:
                    wanted[dep] |= outs
                if dep not in processed or wanted[dep] is None or wanted[dep] - processed[dep]:
                    pending.add(dep)
    return built


def fallback_derivations(paths, required_by):
    """Find valid exact recipes, even when their output paths are not present."""
    paths = sorted(paths)
    drvs = set()
    for i in range(0, len(paths), 200):
        result = subprocess.run(['nix-store', '--query', '--valid-derivers', *paths[i:i + 200]],
                                check=True, capture_output=True, text=True, timeout=180)
        drvs.update(result.stdout.split())
    wanted, matched = {}, set()
    for drv, info in show(drvs).items():
        for name, path in outputs(drv, info).items():
            if path in paths:
                wanted.setdefault(drv, set()).add(name)
                matched.add(path)
    unknown = set(paths) - matched
    if unknown:
        details = [f'{path} (required by {required_by[path]})' for path in sorted(unknown)]
        raise ValueError('Incomplete runtime cache coverage; no valid exact derivation for:\n'
                         + '\n'.join(details))
    return wanted


def check_runtime(coverage):
    built, handled = {}, set()
    while paths := coverage.runtime_fallbacks() - handled:
        wanted = fallback_derivations(paths, coverage.required_by)
        built.update(builds_for(wanted, coverage, force=paths))
        handled.update(paths)
    print(f'runtime: {len(coverage.references)} available archives; {len(handled)} build fallbacks')
    return built


def main(argv):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--allow', action='append', default=[])
    parser.add_argument('--cache', action='append', help='override the configured binary caches')
    parser.add_argument('installables', nargs='+')
    args = parser.parse_args(argv)
    allowed = set(args.allow)
    coverage = CacheCoverage(args.cache if args.cache is not None else CACHES)
    roots = nix('path-info', '--derivation', *args.installables).split()
    built = builds_for(dict.fromkeys(roots), coverage)
    built.update(check_runtime(coverage))
    kinds = {'local': [], 'downloads': [], 'copies': [], 'compiles': []}
    for drv, info in built.items():
        kinds[kind(info)].append(info['name'])
    for name, items in kinds.items():
        print(f'{name}: {len(items)}')
        if name != 'local':
            for item in sorted(items):
                print(f'  {item}{"  (allowed)" if item in allowed else ""}')
    compiles = [item for item in kinds['compiles'] if item not in allowed]
    if compiles:
        print(f'Would compile {len(compiles)}: {", ".join(sorted(compiles))}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main(sys.argv[1:]))
    except subprocess.CalledProcessError as error:
        sys.exit(error.stderr.strip() or str(error))
    except (ValueError, subprocess.TimeoutExpired) as error:
        sys.exit(str(error))
