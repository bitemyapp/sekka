#!/usr/bin/env python3
"""What a Sekka system with an empty store would build rather than download.

Usage: scripts/check_cache_coverage.py [--allow NAME]... INSTALLABLE...

Starting from each installable's derivation, follow what none of the caches
Sekka systems use has, the way Nix would: a derivation whose wanted outputs
are all in a cache is downloaded; one that isn't is built, which needs its
inputs. The derivations that would be built are listed in three kinds:

- local: meant to be built where they are used (preferLocalBuild, or
  allowSubstitutes off), such as configuration files: quick by design;
- downloads: fixed-output derivations, which fetch their sources;
- copies: built without a C compiler (stdenvNoCC) or any other compiler or
  build tool among their inputs, such as Determinate Nixd put in place from
  its download;
- compiles: everything else.

It exits with 1 when anything would be compiled, other than derivations
named with --allow (by name without the hash and `.drv`). The nightly
workflow runs it before moving `nightly`, so updates never compile.
"""
import json
import subprocess
import sys

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


def uncached(paths):
    """Of these store paths, those no cache has."""
    missing = sorted(set(paths))
    for cache in CACHES:
        if not missing:
            break
        out = subprocess.run(['nix-store', '--store', cache, '--check-validity', '--print-invalid', *missing],
                             check=True, capture_output=True, text=True).stdout
        missing = out.split()
    return set(missing)


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


def main(argv):
    allowed, installables = set(), []
    args = iter(argv)
    for arg in args:
        if arg == '--allow':
            allowed.add(next(args))
        else:
            installables.append(arg)
    if not installables:
        sys.exit(__doc__)
    roots = nix('path-info', '--derivation', *installables).split()
    wanted = {root: None for root in roots}  # None: every output
    seen, built = set(), {}
    pending = list(roots)
    while pending:
        infos = show(set(pending) - seen)
        seen |= set(pending)
        paths = {drv: outputs(drv, info) for drv, info in infos.items()}
        needed = [path for drv, outs in paths.items() for name, path in outs.items()
                  if wanted.get(drv) is None or name in wanted[drv]]
        missing = uncached(needed)
        pending = []
        for drv, info in infos.items():
            if not any(path in missing for name, path in paths[drv].items()
                       if wanted.get(drv) is None or name in wanted[drv]):
                continue
            built[drv] = info
            for dep, want in info['inputs']['drvs'].items():
                dep = dep if dep.startswith(STORE) else STORE + dep
                outs = set(want.get('outputs', want) if isinstance(want, dict) else want)
                if dep in seen:
                    continue
                wanted[dep] = (wanted.get(dep) or set()) | outs
                pending.append(dep)
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
    sys.exit(main(sys.argv[1:]))
