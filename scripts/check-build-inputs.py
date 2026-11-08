#!/usr/bin/env python3
"""Exercise Nix source identities across metadata, source, and worktree changes."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--source-filter", type=Path)
    parser.add_argument("--launch-probe", type=Path)
    parser.add_argument("--nixpkgs", type=Path, help="Pre-realized root-lock Nixpkgs source for offline checks")
    parser.add_argument("--store", help="Optional private Nix store for sandboxed checks")
    parser.add_argument("--results", type=Path)
    args = parser.parse_args()
    repo = args.repo.resolve()
    source_filter = (args.source_filter or repo / "nix/bazel/without-build-files.nix").resolve()
    launch_probe = (args.launch_probe or repo / "nix/scheduling/aizome-launch-probe.nix").resolve()
    rows = []

    def evaluate(label, source, kind):
        pkgs_source = args.nixpkgs.resolve() if args.nixpkgs else repo / "nix/bazel/nixpkgs.nix"
        pkgs_args = "{ config = {}; overlays = []; }" if args.nixpkgs else "{}"
        pkgs = f"import (builtins.toPath {json.dumps(str(pkgs_source))}) {pkgs_args}"
        root = f"/. + {json.dumps(str(source))}"
        if kind == "source":
            body = f"""
              let pkgs = {pkgs}; root = {root};
                  filter = import (builtins.toPath {json.dumps(str(source_filter))}) {{ inherit (pkgs) lib; }};
              in toString (pkgs.lib.fileset.toSource {{
                inherit root; fileset = filter [ root ] root;
              }})
            """
        else:
            body = f"""
              let pkgs = {pkgs}; probe = import (builtins.toPath {json.dumps(str(launch_probe))}) {{
                inherit pkgs; source = {root};
              }};
              in probe.drvPath
            """
        command = ["nix", "eval", "--impure", "--json", "--expr", body]
        if args.store:
            command += ["--store", args.store]
        result = subprocess.run(command, text=True, capture_output=True)
        row = {"label": label, "kind": kind, "argv": command, "exit_status": result.returncode,
               "stderr": result.stderr, "identity": json.loads(result.stdout) if result.returncode == 0 else None}
        rows.append(row)
        if result.returncode:
            raise RuntimeError(f"{label}: {result.stderr}")
        return row["identity"]

    assertions = []

    def check(label, actual, expected, same=True):
        passed = (actual == expected) == same
        assertions.append({"label": label, "passed": passed, "actual": actual, "reference": expected,
                           "expected_relation": "equal" if same else "different"})

    with tempfile.TemporaryDirectory(prefix="sekka-build-inputs-") as temporary:
        directory = Path(temporary)
        source = directory / "workspace"
        (source / "src").mkdir(parents=True)
        manifest = source / "Cargo.toml"
        manifest.write_text('[package]\nname="input-check"\nversion="0.1.0"\nedition="2024"\n')
        rust = source / "src/lib.rs"
        rust.write_text("pub fn value() -> u32 { 1 }\n")
        baseline = evaluate("package-baseline", source, "source")
        for name in ("BUILD.bazel", "BUCK", "PACKAGE", ".buckconfig", ".buckrootconfig", "buckal.toml"):
            (source / "src" / name).write_text("generated build graph metadata\n")
        check("generated-metadata-does-not-change-package-source", evaluate("package-metadata", source, "source"), baseline)
        rust.write_text("pub fn value() -> u32 { 2 }\n")
        check("rust-source-invalidates-package-source", evaluate("package-rust-edit", source, "source"), baseline, same=False)
        rust.write_text("pub fn value() -> u32 { 1 }\n")
        manifest.write_text(manifest.read_text() + '[features]\nextra=[]\n')
        check("cargo-manifest-invalidates-package-source", evaluate("package-manifest-edit", source, "source"), baseline, same=False)

        session = repo / "desktop/aizome/crates/aizome-comp-core/src/session.rs"
        original_session = session.read_bytes()
        roots = [directory / "checkout-a", directory / "checkout-b"]
        for root in roots:
            target = root / "crates/aizome-comp-core/src/session.rs"
            target.parent.mkdir(parents=True)
            target.write_bytes(original_session)
        baseline_probe = evaluate("probe-baseline", roots[0], "probe")
        check("independent-worktree-preserves-probe", evaluate("probe-independent-tree", roots[1], "probe"), baseline_probe)
        (roots[0] / "BUCK").write_text("generated build graph metadata\n")
        (roots[0] / "unrelated.rs").write_text("pub fn unrelated() {}\n")
        check("unrelated-files-preserve-probe", evaluate("probe-unrelated-files", roots[0], "probe"), baseline_probe)
        target = roots[0] / "crates/aizome-comp-core/src/session.rs"
        target.write_bytes(original_session + b"\n// changed probe input\n")
        check("actual-probe-source-invalidates", evaluate("probe-source-edit", roots[0], "probe"), baseline_probe, same=False)
        target.write_bytes(original_session)
        check("probe-source-revert-restores-identity", evaluate("probe-source-revert", roots[0], "probe"), baseline_probe)

    report = {"passed": all(row["passed"] for row in assertions), "assertions": assertions, "evaluations": rows}
    encoded = json.dumps(report, indent=2) + "\n"
    if args.results:
        args.results.parent.mkdir(parents=True, exist_ok=True)
        args.results.write_text(encoded)
    print(encoded)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
