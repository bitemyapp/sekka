# The root flake supplies pkgs: no independent toolchain lock or checkout source.
{ pkgs, rustOverlay ? null }:
let
  inherit (pkgs) lib;
  shared = import ../bazel/toolchains.nix { inherit pkgs; };
  buck2 = import ./engine.nix { inherit pkgs rustOverlay; };
  scripts = lib.fileset.toSource {
    root = ../../build/buck2;
    fileset = lib.fileset.unions [
      ../../build/buck2/client.py
      ../../build/buck2/cas_dedupe.py
      ../../build/buck2/generate.py
      ../../build/buck2/action.py
      ../../build/buck2/run.py
      ../../build/buck2/CARGO-LICENSE-MIT
    ];
  };
  backendTestSource = lib.fileset.toSource {
    root = ../../build/buck2;
    fileset = lib.fileset.unions [
      ../../build/buck2/client.py
      ../../build/buck2/generate.py
      ../../build/buck2/action.py
      ../../build/buck2/run.py
      ../../build/buck2/test_backend.py
      ../../build/buck2/cas_dedupe.py
      ../../build/buck2/test_cas_dedupe.py
      ../../build/buck2/units.bzl
      ../../build/buck2/platforms.bzl
      ../../build/buck2/CARGO-LICENSE-MIT
    ];
  };
  vendor = pkgs.rustPlatform.importCargoLock {
    lockFile = ../../desktop/aizome/Cargo.lock;
    # Cargo.lock already pins every Git revision. Fetch those exact trees
    # without a second, hand-maintained per-crate hash/version inventory.
    allowBuiltinFetchGit = true;
  };
  environment = pkgs.runCommand "sekka-buck2-environment" {
    nativeBuildInputs = [ pkgs.jq ];
  } ''
    mkdir -p $out
    jq '. + $tools' ${shared.native}/env.json --argjson tools '${builtins.toJSON {
      rustc = "${shared.rust}/bin/rustc";
      cargo = "${shared.rust}/bin/cargo";
      cc = "${shared.cc}/bin/cc";
      cxx = "${pkgs.stdenv.cc}/bin/c++";
      ar = "${shared.cc}/bin/ar";
      mold = "${pkgs.mold}/bin/mold";
      python = "${pkgs.python3}/bin/python3";
      target = pkgs.stdenv.hostPlatform.rust.rustcTarget;
      rustlib = "${shared.rust}/lib/rustlib/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/lib";
    }}' > $out/env.json
    cat > $out/buckconfig <<EOF
    [sekka]
      python = ${pkgs.python3}/bin/python3
      environment = $out/env.json
    EOF
  '';
  generator = pkgs.writeShellScriptBin "sekka-buck2-generate" ''
    export SEKKA_BUCK2_ENV=${environment}/env.json
    export SEKKA_BUCK2_VENDOR=${vendor}
    exec ${pkgs.python3}/bin/python3 ${scripts}/generate.py "$@"
  '';
  client = pkgs.writeShellScriptBin "sekka-buck2" ''
    export SEKKA_BUCK2_ENV=${environment}/env.json
    export SEKKA_BUCK2_VENDOR=${vendor}
    export SEKKA_BUCK2_CONFIG=${environment}/buckconfig
    export SEKKA_BUCK2_BIN=${buck2}/bin/buck2
    export SEKKA_BUCK2_RUNTIME=${shared.testEnv}/env.json
    exec ${pkgs.python3}/bin/python3 ${scripts}/client.py "$@"
  '';
  backendTest = pkgs.runCommand "sekka-buck2-backend-test" {
    SEKKA_BUCK2_ENV = "${environment}/env.json";
    SEKKA_BUCK2_BIN = "${buck2}/bin/buck2";
    SEKKA_BUCK2_RUNTIME = "${shared.testEnv}/env.json";
    # Buck initializes its HTTP client even for this network-disabled local build.
    SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
    PYTHONDONTWRITEBYTECODE = "1";
  } ''
    ${pkgs.python3}/bin/python3 ${backendTestSource}/test_cas_dedupe.py
    ${pkgs.python3}/bin/python3 ${backendTestSource}/test_backend.py
    touch "$out"
  '';
in {
  inherit buck2;
  inherit vendor environment generator client backendTest;
  inherit (shared) native rust cc;
  runtime = shared.testEnv;
  shell = pkgs.mkShell {
    packages = [ client generator buck2 pkgs.python3 shared.rust ];
    SEKKA_BUCK2_ENV = "${environment}/env.json";
    SEKKA_BUCK2_VENDOR = toString vendor;
    SEKKA_BUCK2_CONFIG = "${environment}/buckconfig";
    SEKKA_BUCK2_BIN = "${buck2}/bin/buck2";
    SEKKA_BUCK2_RUNTIME = "${shared.testEnv}/env.json";
  };
}
