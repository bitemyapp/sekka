# Realized once per declared Nix boundary by scripts/sekka-bazel. This is a
# rooted executable closure, not a saved shell environment or repository snapshot.
{ pkgs ? import ./nixpkgs.nix { } }:
let
  toolchains = import ./toolchains.nix { inherit pkgs; };
  repositories = import ./repositories.nix { inherit pkgs; };
  frontend = pkgs.runCommand "sekka-bazel-frontend" { } ''
    mkdir -p $out
    cp ${../../build/bazel/workflow.py} $out/workflow.py
    cp ${../../build/bazel/credential_helper.py} $out/credential_helper.py
  '';
  credentialHelper = pkgs.writeShellScript "sekka-bazel-credentials" ''
    exec ${pkgs.python3}/bin/python3 ${frontend}/credential_helper.py "$@"
  '';
  # Do not retain credentials for Bazel's default thirty minutes after a
  # same-path reader/writer token rotation. The helper reads private file data.
  credentialsRc = pkgs.writeText "sekka-bazel-credentials.bazelrc" ''
    common --credential_helper=${credentialHelper}
    common --credential_helper_cache_duration=0s
  '';
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
  export SEKKA_BAZEL_CREDENTIALS_RC=${credentialsRc}
  exec ${pkgs.python3}/bin/python3 ${frontend}/workflow.py "$@"
''