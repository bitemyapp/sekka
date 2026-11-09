#!/usr/bin/env python3
"""Evaluate real package identities after edits in a disposable source tree.

Run from any directory with Python 3 and Nix available. Does not build packages
or modify the checkout. This complements, rather than replaces, package/VM tests.
"""
import json
import shutil
import subprocess
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
ROOTS = [
    "desktop/aizome/Cargo.toml", "desktop/aizome/Cargo.lock",
    "desktop/aizome/crates", "desktop/aizome/apps", "desktop/aizome/icons",
    "desktop/aizome/etc", "desktop/aizome/session", "desktop/aizome/scripts/rustc-wrapper",
    "desktop/aizome/sukumo/Cargo.toml", "desktop/aizome/sukumo/crates",
    "apps/yukimi/Cargo.toml", "apps/yukimi/crates",
    "apps/hotaru/Cargo.toml", "apps/hotaru/crates", "apps/hotaru/vendor/simdutf8",
    "apps/hotaru/resources", "apps/hotaru/upstream/LICENSE.ghostty",
    "apps/hotaru/upstream/LICENSE.uucode", "apps/hotaru/upstream/LICENSE.unicode",
]


def identities(root, different_fonts=False):
    fonts = ('public.aizome-fonts.overrideAttrs (_: { name = "font-identity-probe"; })'
             if different_fonts else 'public.aizome-fonts')
    expression = f'''
      let
        flake = builtins.getFlake {json.dumps(str(REPO))};
        pkgs = flake.inputs.nixpkgs.legacyPackages.x86_64-linux;
        public = flake.packages.x86_64-linux;
        components = import {REPO}/desktop/aizome/nix/package.nix {{
          inherit pkgs;
          craneLib = flake.inputs.crane.mkLib pkgs;
          fonts = {fonts};
          hotaruFonts = pkgs.callPackage {REPO}/desktop/aizome/nix/hotaru-fonts.nix {{ }};
          root = /. + {json.dumps(str(root))};
        }};
      in builtins.mapAttrs (_: package: package.drvPath)
        ((builtins.removeAttrs components [ "sources" ]) // {{
          appDependencies = components.aggregate.compilerArtifacts.appDependencies;
          toolkitLibraries = components.aggregate.compilerArtifacts.kitArtifacts;
          calculatorLibraries = components.soroban.cargoArtifacts;
          compositorDependencies = components.compositor.cargoArtifacts;
        }})
    '''
    result = subprocess.run(["nix", "eval", "--json", "--impure", "--expr", expression],
                            text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(result.stderr)
    return json.loads(result.stdout)


def main():
    with tempfile.TemporaryDirectory(prefix="aizome-package-boundaries-") as directory:
        root = Path(directory)
        for relative in ROOTS:
            source, destination = REPO / relative, root / relative
            destination.parent.mkdir(parents=True, exist_ok=True)
            if source.is_dir():
                shutil.copytree(source, destination, symlinks=True)
            else:
                shutil.copy2(source, destination)
        baseline = identities(root)
        cases = [
            ("calculator source", "desktop/aizome/apps/soroban/src/main.rs", b"\n// identity probe\n", {"soroban", "aggregate"}),
            ("calculator manifest", "desktop/aizome/apps/soroban/Cargo.toml", b"\n# identity probe\n", {"soroban", "aggregate"}),
            ("clock data", "desktop/aizome/apps/tokei/data/cities.tsv", b"\n", {"tokei", "aggregate"}),
            ("terminal integration", "apps/hotaru/resources/shell-integration/bash/hotaru.bash", b"\n# identity probe\n", {"hotaru", "aggregate"}),
        ]
        for label, relative, suffix, expected in cases:
            file = root / relative
            original = file.read_bytes()
            file.write_bytes(original + suffix)
            changed = {name for name, value in identities(root).items() if value != baseline[name]}
            assert changed == expected, (label, changed, expected)
            file.write_bytes(original)
            assert identities(root) == baseline, f"{label}: reverting did not restore identities"
            print(f"PASS {label}: changed only {', '.join(sorted(changed))}")

        changed = {name for name, value in identities(root, different_fonts=True).items()
                   if value != baseline[name]}
        expected = {"aggregate", "session", "hako", "hotaru", "shell", "yukimi"}
        assert changed == expected, ("font assets", changed, expected)
        print("PASS font assets: only wrappers/check consumers change; compiler caches stay stable")

        search_source = root / "desktop/aizome/crates/aizome-search/src/lib.rs"
        original = search_source.read_bytes()
        search_source.write_bytes(original + b"\n// private dependency probe\n")
        changed = identities(root)
        for name in ("appDependencies", "toolkitLibraries", "compositor"):
            assert changed[name] == baseline[name], f"search library invalidated {name}"
        for name in ("calculatorLibraries", "soroban"):
            assert changed[name] != baseline[name], f"search library did not invalidate {name}"
        search_source.write_bytes(original)
        assert identities(root) == baseline, "reverting search library did not restore identities"
        print("PASS private dependency: preserves shared toolkit and unrelated compositor")

        kit_build = root / "desktop/aizome/crates/aizome-kit/build.rs"
        kit_build.write_bytes(kit_build.read_bytes() + b"\n// dependency build-script probe\n")
        changed = identities(root)
        assert changed["appDependencies"] == baseline["appDependencies"], "kit build.rs invalidated external dependencies"
        assert changed["toolkitLibraries"] != baseline["toolkitLibraries"], "kit build.rs did not invalidate shared toolkit"
        assert changed["soroban"] != baseline["soroban"], "kit build.rs did not invalidate Soroban"
        assert changed["compositor"] == baseline["compositor"], "kit build.rs invalidated the unrelated compositor"
        print("PASS dependency build script: invalidates its consumers, preserves compositor")


if __name__ == "__main__":
    main()
