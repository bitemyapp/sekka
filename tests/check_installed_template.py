#!/usr/bin/env python3
"""Validate the real backend against the generated lock and evaluate a target.

Build the fixture first; run from the repository root:
cargo build --locked --manifest-path installer/genkan/tests/vm-fixture/Cargo.toml --target-dir .work/fixture-target
python3 tests/check_installed_template.py
"""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = Path(os.environ.get('CALAMARES_FIXTURE', ROOT / '.work/fixture-target/debug/calamares-vm-fixture'))


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True)


lock = command('nix', 'eval', '--json', '.#lib.installedLock')
parsed = json.loads(lock)
assert 'sekka' in parsed['nodes'][parsed['root']]['inputs']
assert not {'calamares', 'tatami', 'yukimi', 'aizome', 'sidestep'} & parsed['nodes'][parsed['root']]['inputs'].keys()
# Aizome is Sekka's own source; Sidestep, which it builds against, stays
# pinned beneath Sekka.
assert 'aizome' not in parsed['nodes']
assert 'sidestep' in parsed['nodes']['sekka']['inputs']
cases = json.loads(command(str(FIXTURE), 'desktop-configurations'))
cases.update(json.loads(command(str(FIXTURE), 'storage-configurations')))
with tempfile.TemporaryDirectory(prefix='sekka-installed-template-') as directory:
    target = Path(directory)
    (target / 'flake.lock').write_text(lock)
    template = command(str(FIXTURE), 'validate-template', str(ROOT / 'templates/flake.nix.in'), str(target / 'flake.lock'))
    (target / 'flake.nix').write_text(template)
    # These devices are never opened: evaluation only, no builds or activation.
    (target / 'hardware-configuration.nix').write_text('{ ... }: {}\n')
    for name in ['aizome', 'aizome-tatami', 'aizome-plasma-gnome-xfce-hyprland-tatami',
                 'plasma', 'tatami', 'gnome-xfce', 'encrypted-uefi', 'encrypted-bios']:
        (target / 'configuration.nix').write_text(cases[name])
        derivation = command('nix', 'eval', '--no-update-lock-file', '--raw',
                             f'path:{target}#nixosConfigurations.sekka-test.config.system.build.toplevel.drvPath')
        assert derivation.endswith('.drv'), derivation
        assert (target / 'flake.lock').read_text() == lock
        print(f'PASS: installed template, backend validator, unchanged lock, {name}: {derivation}')
