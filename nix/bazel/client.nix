# Realized once per declared Nix boundary by scripts/sekka-bazel. This is a
# rooted executable closure, not a saved shell environment or repository snapshot.
{ pkgs ? import ./nixpkgs.nix { } }:
let
  toolchains = import ./toolchains.nix { inherit pkgs; };
  repositories = import ./repositories.nix { inherit pkgs; };
  bazel = pkgs.writeShellScriptBin "bazel" ''
    exec ${pkgs.bazel_9}/bin/bazel --nosystem_rc --nohome_rc \
      --server_javabase=${pkgs.jdk21_headless} "$@"
  '';
in
pkgs.writeShellScriptBin "sekka-bazel-client" ''
  export PATH=${pkgs.lib.makeBinPath [ bazel pkgs.python3 toolchains.rust pkgs.nix pkgs.git pkgs.coreutils ]}:"$PATH"
  export SEKKA_BAZEL_RUST_TOOLCHAIN=${toolchains.rust}
  export SEKKA_BAZEL_NATIVE_ENV=${toolchains.native}
  export SEKKA_BAZEL_RUNTIME_ENV=${toolchains.testEnv}
  export SEKKA_BAZEL_REPOSITORIES_RC=${repositories}
  exec ${pkgs.python3}/bin/python3 ${../../build/bazel/workflow.py} "$@"
''