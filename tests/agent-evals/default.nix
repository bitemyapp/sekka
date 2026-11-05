# The agent evaluations (README.md): one NixOS VM test whose script runs a
# single episode, configured from outside ($SEKKA_EVAL_CONFIG), and the
# host-side runner that boots many of them against a local model server.
#
#     nix build .#agent-evals.driver -o .work/evals/driver
#     nix run .#agent-evals.run -- --help
{
  pkgs,
  desktopsModule,
  localModels,
  ompPackage ? null,
}:
let
  lib = pkgs.lib;
  # The laptop's key in the SSH scenario: a test fixture, made at build time.
  sshKeys = pkgs.runCommand "agent-eval-ssh-keys" { nativeBuildInputs = [ pkgs.openssh ]; } ''
    mkdir $out
    ssh-keygen -q -t ed25519 -N "" -C alice@laptop -f $out/id_ed25519
  '';
  harness = lib.fileset.toSource {
    root = ./harness;
    fileset = lib.fileset.fileFilter (f: f.hasExt "py") ./harness;
  };
  test = pkgs.testers.runNixOSTest {
    name = "sekka-agent-eval";
    globalTimeout = 3 * 3600;
    node.pkgsReadOnly = false;
    nodes.machine = import ./machine.nix { inherit desktopsModule sshKeys ompPackage; };
    skipLint = true;
    skipTypeCheck = true;
    # $SEKKA_EVAL_HARNESS runs a checkout's harness without rebuilding.
    testScript = ''
      import os, sys
      sys.path.insert(0, os.environ.get("SEKKA_EVAL_HARNESS") or "${harness}")
      import episode
      episode.run(machine)
    '';
  };
  run = pkgs.writeShellApplication {
    name = "sekka-agent-evals";
    runtimeInputs = [
      pkgs.python3
      pkgs.coreutils
      pkgs.util-linux
      localModels.serve
    ];
    text = ''
      export SEKKA_EVAL_DRIVER=''${SEKKA_EVAL_DRIVER:-${test.driver}/bin/nixos-test-driver}
      export SEKKA_MODEL_SERVER=sekka-model-server
      exec python3 -I ${./run.py} "$@"
    '';
  };
in
{
  inherit test harness run;
  inherit (test) driver driverInteractive;
}
