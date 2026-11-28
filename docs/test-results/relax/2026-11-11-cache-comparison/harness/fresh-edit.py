#!/usr/bin/env python3
"""One fresh-output worktree with a previously unseen, executed source edit."""
import ast
import hashlib
import json
import os
from pathlib import Path
import subprocess

B = Path('/home/callen/sekka-work/relax-cache-comparison-20261111')
E = B / 'evidence'
data = json.loads((E / 'results.json').read_text())
assert data['status'] == 'passed'
tree = Path('/home/callen/sekka-work/buck2-final-20261106/run/trees/release-fresh')
assert subprocess.check_output(['git', '-C', str(tree), 'rev-parse', 'HEAD'], text=True).strip() == data['trees'][0]['head']
assert subprocess.check_output(['git', '-C', str(tree), 'status', '--porcelain', '--untracked-files=no'], text=True) == ''
assert not (tree / 'buck-out/cache-compare-20261111').exists()

# Reuse the exact timing/storage functions without rerunning the first plan's
# initialization or scenarios. Their source is retained beside this file.
source = ast.parse((B / 'compare.py').read_text())
selected = [node for node in source.body if isinstance(node, (ast.Import, ast.ImportFrom, ast.FunctionDef))]
scope = {}
exec(compile(ast.Module(body=selected, type_ignores=[]), str(B / 'compare.py'), 'exec'), scope)
scope.update(BASE=B, OUT=E, DATA=data, TREES=[Path(t['path']) for t in data['trees']] + [tree],
             ENV=dict(os.environ, **data['environment']),
             RELAX=Path('/home/callen/sekka-work/relax-followup-20261111/relax'),
             SERVICE=Path('/home/callen/sekka-work/shared-build-service-39107ce52f3b'),
             CONTROLLER='/nix/store/r21z8qj4ajl30kwxmag5b6ma7bfj863v-sekka-build-service/bin/sekka-build-service',
             COMPSIZE='/nix/store/iq0ifcxdi8hisakrh6qc5iy0mm3s285g-compsize-1.5-unstable-2023-12-24/bin/compsize',
             ISOLATION='cache-compare-20261111',
             PACKAGES=[('aizome-kit', '//desktop/aizome/crates/aizome-kit:aizome_kit-test-test-lib'),
                       ('aizome-soroban', '//desktop/aizome/apps/soroban:aizome_soroban-test-test-lib')])
assert scope['ctypes'].CDLL(None).prctl(36, 1, 0, 0, 0) == 0
scope['snapshot']('before-fresh-edit-d')
path = tree / 'desktop/aizome/apps/soroban/src/lib.rs'
original = path.read_bytes()
edit = b'\n#[cfg(test)]\n#[test]\nfn cache_comparison_new_worktree_probe() { assert_eq!(std::hint::black_box(11_u64).wrapping_add(2), 13); }\n'
try:
    path.write_bytes(original + edit)
    scope['pair']('fresh-leaf-edit-d', tree)
    scope['snapshot']('fresh-leaf-edit-d')
    scope['pair']('fresh-leaf-warm-d', tree, 3)
finally:
    path.write_bytes(original)
    data['restored_sources'][str(path)] = hashlib.sha256(path.read_bytes()).hexdigest()
    data['trees'].append({'path': str(tree), 'head': data['trees'][0]['head'], 'tracked_status': ''})
    scope['save']()

# Update the metadata and export after this additive follow-up. This does not
# repeat or replace any timing observation from the original plan.
metadata = json.loads((E / 'metadata.json').read_text())
metadata['source_status'].append({'tree': str(tree), 'tracked_status': subprocess.check_output(
    ['git', '-C', str(tree), 'status', '--porcelain', '--untracked-files=no'], text=True),
    'manifest_lock_hashes': metadata['source_status'][0]['manifest_lock_hashes']})
assert metadata['source_status'][-1]['tracked_status'] == ''
(E / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
export_source = (B / 'finalize.py').read_text().split('# Preserve raw hashes', 1)[1]
export_source = export_source.replace('dest.mkdir()', 'dest.mkdir(exist_ok=True)')
namespace = dict(B=B, E=E, Path=Path, json=json, hashlib=hashlib,
                 manifest=json.loads((B / 'service.json').read_text()))
import gzip
namespace['gzip'] = gzip
exec(compile('# Preserve raw hashes' + export_source, str(B / 'finalize.py'), 'exec'), namespace)
