#!/usr/bin/env python3
"""Validate the real backend against the generated lock and evaluate a target.

Build the fixture first; run from the repository root:
cargo build --locked --manifest-path installer/genkan/tests/vm-fixture/Cargo.toml --target-dir .work/fixture-target
python3 tests/check_installed_template.py
"""
import json
import os
import platform
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
FIXTURE = Path(os.environ.get('GENKAN_VM_FIXTURE', ROOT / '.work/fixture-target/debug/calamares-vm-fixture'))
SYSTEM = {'arm64': 'aarch64-linux', 'aarch64': 'aarch64-linux', 'x86_64': 'x86_64-linux'}[platform.machine()]


def command(*args):
    return subprocess.check_output(args, cwd=ROOT, text=True)


lock = command('nix', 'eval', '--json', '.#lib.installedLock')
parsed = json.loads(lock)
assert 'sekka' in parsed['nodes'][parsed['root']]['inputs']
assert not {'calamares', 'tatami', 'yukimi'} & parsed['nodes'][parsed['root']]['inputs'].keys()
cases = json.loads(command(str(FIXTURE), 'desktop-configurations'))
with tempfile.TemporaryDirectory(prefix='sekka-installed-template-') as directory:
    target = Path(directory)
    (target / 'flake.lock').write_text(lock)
    template = command(str(FIXTURE), 'validate-template', str(ROOT / 'templates/flake.nix.in'), str(target / 'flake.lock'))
    (target / 'flake.nix').write_text(template)
    # These devices are never opened: evaluation only, no builds or activation.
    (target / 'hardware-configuration.nix').write_text('{ ... }: {}\n')
    for name in ['plasma', 'tatami', 'gnome-xfce']:
        (target / 'configuration.nix').write_text(cases[name])
        host = json.loads(command('nix', 'eval', '--no-update-lock-file', '--json',
            f'path:{target}#nixosConfigurations.sekka-test', '--apply',
            'host: { system = host.pkgs.stdenv.hostPlatform.system; '
            'derivation = host.config.system.build.toplevel.drvPath; '
            'alsa32 = host.config.services.pipewire.alsa.support32Bit; '
            'qemu = host.config.services.qemuGuest.enable; '
            'spice = host.config.services.spice-vdagentd.enable; }'))
        derivation = host['derivation']
        assert derivation.endswith('.drv'), derivation
        assert host['system'] == SYSTEM, host
        assert host['alsa32'] == (SYSTEM == 'x86_64-linux'), host
        if SYSTEM == 'aarch64-linux':
            assert host['qemu'] and host['spice'], host
        assert (target / 'flake.lock').read_text() == lock
        print(f'PASS: installed template, native {SYSTEM}, backend validator, unchanged lock, {name}: {derivation}')
