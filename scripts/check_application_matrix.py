#!/usr/bin/env python3
"""Evaluate actual backend-generated configurations, one Nix process per case.

Run on Linux after building the local calamares-vm-fixture into
.work/native-fixture/local/bin. Nix and that fixture must be executable.
"""
import hashlib
import json
import pathlib
import subprocess


def main():
    repo = pathlib.Path(__file__).resolve().parents[1]
    revision = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=repo, text=True).strip()
    fixture = repo / '.work/native-fixture/local/bin/calamares-vm-fixture'
    raw = subprocess.check_output([str(fixture), 'application-configurations'])
    cases = json.loads(raw)
    modules = repo / '.work/application-modules'
    modules.mkdir(parents=True, exist_ok=True)
    for name, source in cases.items():
        if not name or any(c not in 'abcdefghijklmnopqrstuvwxyz-' for c in name):
            raise ValueError(f'Invalid fixture case: {name!r}')
        (modules / (name + '.nix')).write_text(source.replace(
            'imports = [ ./hardware-configuration.nix ];', ''))
    results = {}
    for name in sorted(cases):
        result = subprocess.check_output([
            'nix', 'eval', '--impure', '--json', '--file',
            'tests/application-matrix.nix', '--apply',
            f'f: f {{ caseName = "{name}"; }}',
        ], cwd=repo)
        results[name] = json.loads(result)
        print('PASS', name, flush=True)
    output = {
        'passed': True,
        'sekka_revision': revision,
        'flake_lock_sha256': hashlib.sha256((repo / 'flake.lock').read_bytes()).hexdigest(),
        'generated_configurations_sha256': hashlib.sha256(raw).hexdigest(),
        'cases': results,
    }
    destination = repo / 'docs/test-results/application-configurations.json'
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(json.dumps(output, indent=2) + '\n')
    print('PASS all', len(cases), 'application configurations', flush=True)


if __name__ == '__main__':
    main()
