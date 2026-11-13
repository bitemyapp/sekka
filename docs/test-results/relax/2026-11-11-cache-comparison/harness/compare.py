#!/usr/bin/env python3
"""Matched component tests with fresh writable caches and shared worktree caches."""
import ctypes
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import threading
import time

BASE = Path('/home/callen/sekka-work/relax-cache-comparison-20261111')
TREES = [Path(p) for p in (
    '/home/callen/sekka-work/relax-experiment-20261111/buck2',
    '/home/callen/sekka-work/buck2-final-20261106/repo',
    '/home/callen/sekka-work/buck2-final-20261106/run/trees/a')]
RELAX = Path('/home/callen/sekka-work/relax-followup-20261111/relax')
SERVICE = Path('/home/callen/sekka-work/shared-build-service-39107ce52f3b')
CONTROLLER = '/nix/store/r21z8qj4ajl30kwxmag5b6ma7bfj863v-sekka-build-service/bin/sekka-build-service'
COMPSIZE = '/nix/store/iq0ifcxdi8hisakrh6qc5iy0mm3s285g-compsize-1.5-unstable-2023-12-24/bin/compsize'
ISOLATION = 'cache-compare-20261111'
PACKAGES = [('aizome-kit', '//desktop/aizome/crates/aizome-kit:aizome_kit-test-test-lib'),
            ('aizome-soroban', '//desktop/aizome/apps/soroban:aizome_soroban-test-test-lib')]
OUT = BASE / 'evidence'
OUT.mkdir(exist_ok=True)
# Reap optional native Cargo work orphaned when the Relax foreground process exits.
assert ctypes.CDLL(None).prctl(36, 1, 0, 0, 0) == 0
ENV = dict(os.environ, CARGO_BUILD_JOBS='16', MAKEPAD_CPU_CORES='16',
           MAKEPAD_RUN_STD=str(RELAX / 'libs/relax/compiler/std'),
           MAKEPAD_RUN_CACHE_DIR=str(BASE / 'relax-cache'),
           MAKEPAD_RUN_HOME=str(BASE / 'relax-home'),
           MAKEPAD_RUN_OFFLINE='1', CARGO_NET_OFFLINE='true',
           MAKEPAD_RUN_VERBOSE='1',
           SEKKA_BUCK2_CLIENT_CACHE='/home/callen/sekka-work/relax-experiment-20261111/buck2-bootstrap')
DATA = {'schema': 1, 'runs': [], 'snapshots': [], 'trees': [],
        'relax_revision': subprocess.check_output(['git', '-C', str(RELAX), 'rev-parse', 'HEAD'], text=True).strip(),
        'runner_sha256': hashlib.sha256((RELAX / 'target/release/relax').read_bytes()).hexdigest(),
        'rustc': subprocess.check_output(['rustc', '-Vv'], text=True),
        'environment': {k: ENV[k] for k in ENV if k.startswith(('MAKEPAD_', 'CARGO_', 'SEKKA_'))}}
for tree in TREES:
    head = subprocess.check_output(['git', '-C', str(tree), 'rev-parse', 'HEAD'], text=True).strip()
    assert head == '7fef66a8ae5b3f592194dd2104b1afa354ca6dfb', head
    status = subprocess.check_output(['git', '-C', str(tree), 'status', '--porcelain', '--untracked-files=no'], text=True)
    assert not status, status
    assert not (tree / 'buck-out' / ISOLATION).exists(), 'Refusing old Buck writable state'
    DATA['trees'].append({'path': str(tree), 'head': head, 'tracked_status': status})
assert not (BASE / 'relax-cache').exists(), 'Refusing old Relax writable state'

def save():
    (OUT / 'results.json').write_text(json.dumps(DATA, indent=2) + '\n')

def run(stage, tree, engine, repeat=0):
    label = f'{stage}-{engine}-{repeat}'
    print('START', label, flush=True)
    result = {'stage': stage, 'engine': engine, 'repeat': repeat, 'tree': str(tree), 'commands': []}
    for package, target in PACKAGES:
        prefix = OUT / f'{label}-{package}'
        if engine == 'relax':
            command = [str(RELAX / 'target/release/relax'), 'test', '--no-live', '--allow-c', '--offline', package, '--lib']
            cwd = tree / 'desktop/aizome'
        else:
            command = [str(tree / 'scripts/sekka-buck2'), '--isolation-dir', ISOLATION,
                       '--cache', 'cache', '--cache-write', '--config-file', str(BASE / 'writer.rc'),
                       '--local-content-store', str(BASE / 'buck-content-store'), '-j', '16',
                       '--buck-arg=--build-report=' + str(prefix) + '.report.json',
                       '--buck-arg=--event-log=' + str(prefix) + '.events.json-lines.gz',
                       '--buck-arg=--write-build-id=' + str(prefix) + '.build-id', 'test', target]
            cwd = tree
        started = time.monotonic()
        with Path(str(prefix) + '.log').open('w') as log:
            proc = subprocess.Popen(command, cwd=cwd, env=ENV, stdout=log, stderr=subprocess.STDOUT, start_new_session=True)
            def stop():
                try:
                    os.killpg(proc.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            watchdog = threading.Timer(900, stop)
            watchdog.start()
            code = proc.wait()
            foreground = time.monotonic() - started
            orphan_status = []
            if engine == 'relax':
                # Only descendants in this invocation's session; never wait on Buck daemons.
                while True:
                    try:
                        pid, status = os.waitpid(-proc.pid, 0)
                        orphan_status.append({'pid': pid, 'status': status})
                    except ChildProcessError:
                        break
            watchdog.cancel()
        settled = time.monotonic() - started
        row = {'package': package, 'command': command, 'cwd': str(cwd), 'log': str(prefix) + '.log',
               'exit_status': code, 'seconds': foreground, 'settled_seconds': settled,
               'orphan_status': orphan_status}
        result['commands'].append(row)
        print('DONE', label, package, code, round(foreground, 4), round(settled, 4), flush=True)
        if code:
            DATA['runs'].append(result)
            save()
            raise RuntimeError(f'{label}/{package} failed: {prefix}.log')
    result['seconds'] = sum(c['seconds'] for c in result['commands'])
    result['settled_seconds'] = sum(c['settled_seconds'] for c in result['commands'])
    DATA['runs'].append(result)
    save()

def snapshot(stage):
    for engine in ('relax', 'buck2'):
        candidates = ([BASE / 'relax-cache', BASE / 'relax-home'] if engine == 'relax' else
                      [BASE / 'buck-content-store', SERVICE / 'trusted/data'] + [tree / 'buck-out' / ISOLATION for tree in TREES])
        roots = [str(p) for p in candidates if p.exists()]
        prefix = OUT / f'{stage}-{engine}-storage'
        cmd = [CONTROLLER, '--local', '--manifest', str(BASE / 'service.json'), 'physical-inventory',
               '--scope', 'immutable', '--compsize', COMPSIZE]
        for root in roots:
            cmd += ['--allow-root', root]
        cmd += roots
        with prefix.with_suffix('.log').open('w') as log:
            subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT, check=True)
        allocation = subprocess.check_output(['du', '-s', '-B1', *roots], text=True)
        logical = subprocess.check_output(['du', '-s', '-B1', '--apparent-size', *roots], text=True)
        DATA['snapshots'].append({'stage': stage, 'engine': engine, 'roots': roots,
                                  'physical_receipt': str(prefix.with_suffix('.log')),
                                  'allocated_du': allocation, 'logical_du': logical})
        save()
    print('SNAPSHOT', stage, flush=True)

def pair(stage, tree, repetitions=1):
    for repeat in range(repetitions):
        # Alternate first engine so later repeated trials do not always have the same order.
        for engine in (('relax', 'buck2') if repeat % 2 == 0 else ('buck2', 'relax')):
            run(stage, tree, engine, repeat)

originals = {}
def edit(relative, suffix):
    path = TREES[2] / relative
    if path not in originals:
        originals[path] = path.read_bytes()
    path.write_bytes(originals[path] + suffix.encode())

try:
    save()
    pair('cold', TREES[0])
    snapshot('cold')
    pair('warm-a', TREES[0], 3)
    pair('identical-b', TREES[1])
    snapshot('identical-b')
    pair('warm-b', TREES[1], 3)
    edit('README.md', '\n<!-- Worktree cache benchmark: unrelated documentation edit. -->\n')
    pair('unrelated-c', TREES[2])
    snapshot('unrelated-c')
    pair('warm-c', TREES[2], 3)
    edit('desktop/aizome/apps/soroban/src/lib.rs', '\n#[cfg(test)]\n#[test]\nfn cache_comparison_leaf_probe() { assert_eq!(std::hint::black_box(2_u64).wrapping_add(3), 5); }\n')
    pair('leaf-edit-c', TREES[2])
    snapshot('leaf-edit-c')
    pair('leaf-warm-c', TREES[2], 3)
    edit('desktop/aizome/crates/aizome-kit/src/lib.rs', '\n#[inline(never)]\npub fn cache_comparison_shared_probe() -> u64 { std::hint::black_box(2_u64).wrapping_add(3) }\n')
    edit('desktop/aizome/apps/soroban/src/lib.rs', '\n#[cfg(test)]\n#[test]\nfn cache_comparison_shared_probe() { assert_eq!(aizome_kit::cache_comparison_shared_probe(), 5); }\n')
    pair('shared-edit-c', TREES[2])
    snapshot('shared-edit-c')
    pair('shared-warm-c', TREES[2], 3)
    DATA['status'] = 'passed'
finally:
    for path, contents in originals.items():
        path.write_bytes(contents)
    DATA['restored_sources'] = {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in originals}
    save()
