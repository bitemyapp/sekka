# SPDX-License-Identifier: GPL-3.0-or-later
# Sekka's installer engine for Apple Silicon Macs: Asahi's installer v0.9.2,
# pinned by SHA-256 and never copied into this tree, with Sekka's adapter
# (sekka_engine), the J514S hardware profile and engine-lock.json, repacked
# deterministically as sekka-engine-0.9.2-sekka.1.tar.gz.
#
#   archive  the tarball and engine.json (its size and SHA-256)
#   tests    the adapter's unittest suite against a fake Mac, with the
#            pinned installer's own modules, and the contract crate's
#            transcript validator checking what the engine writes
#
# docs/apple-silicon/engine.md describes the engine.
{ pkgs }:
let
  inherit (pkgs) lib;
  fs = lib.fileset;
  python = pkgs.python313;
  name = "sekka-engine-0.9.2-sekka.1";

  # As nix/apple-silicon/install-sekka.sh pins it.
  asahi = pkgs.fetchurl {
    url = "https://cdn.asahilinux.org/installer/installer-v0.9.2.tar.gz";
    hash = "sha256-SE2ZnlO0rvJ9F8RHjklWfpaEL4pss1wHOg31vwu+gnY=";
  };

  python-only = dir: fs.fileFilter (f: f.hasExt "py") dir;

  adapter = fs.toSource {
    root = ./.;
    fileset = fs.unions [
      (python-only ./sekka_engine)
      ./pack.py
    ];
  };

  profile = ../crates/contract/profiles/j514s.json;

  # The contract crate's examples: validate_transcript (the helper's
  # Transcript, line by line) and canonicalize. Building them runs the
  # crate's own tests, including the engine's committed vectors.
  contract = pkgs.rustPlatform.buildRustPackage {
    pname = "sekka-mac-contract-examples";
    version = "0.1.0";
    src = fs.toSource {
      root = ../.;
      fileset = fs.unions [
        ../Cargo.toml
        ../Cargo.lock
        ../crates/contract
        ./tests/vectors
      ];
    };
    cargoLock.lockFile = ../Cargo.lock;
    cargoBuildFlags = [
      "-p"
      "sekka-mac-contract"
      "--examples"
    ];
    cargoTestFlags = [
      "-p"
      "sekka-mac-contract"
    ];
    installPhase = ''
      runHook preInstall
      mkdir -p $out/bin
      for example in validate_transcript canonicalize; do
        install -m755 "$(find target -path "*/release/examples/$example" -type f | head -n1)" $out/bin/$example
      done
      runHook postInstall
    '';
  };

  tests-src = fs.toSource {
    root = ../.;
    fileset = fs.unions [
      (python-only ./sekka_engine)
      (fs.fileFilter (f: !(f.hasExt "pyc")) ./tests)
      profile
    ];
  };
in
{
  inherit contract;

  archive =
    pkgs.runCommand name
      {
        nativeBuildInputs = [ python ];
      }
      ''
        mkdir root
        tar -xzf ${asahi} -C root
        mkdir root/sekka_engine root/sekka-profiles
        cp ${adapter}/sekka_engine/*.py root/sekka_engine/
        cp ${profile} root/sekka-profiles/j514s.json
        chmod -R u+w root
        # -B: importing the adapter to digest it must not leave bytecode in the archive.
        python3 -I -B ${adapter}/pack.py "$PWD/root" ${asahi} $out
      '';

  tests =
    pkgs.runCommand "${name}-tests"
      {
        nativeBuildInputs = [ python ];
      }
      ''
        mkdir asahi
        tar -xzf ${asahi} -C asahi
        cp -r ${tests-src} src
        chmod -R u+w src
        cd src/engine
        export SEKKA_ASAHI_ROOT=$NIX_BUILD_TOP/asahi
        export SEKKA_TRANSCRIPT_VALIDATOR=${contract}/bin/validate_transcript
        python3 -I -m unittest discover -s tests -t tests -v 2>&1 | tee $NIX_BUILD_TOP/tests.log
        test "''${PIPESTATUS[0]}" = 0
        mkdir -p $out
        cp $NIX_BUILD_TOP/tests.log $out/
      '';
}
