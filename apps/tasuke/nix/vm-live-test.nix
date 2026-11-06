# SPDX-License-Identifier: MIT OR Apache-2.0
# The window on Aizome answering through a real model (vm-desktop.nix's
# machine): alice asks a question that only needs looking, the agent runs
# its read-only commands in the VM, and a real model answers. The model
# runs on the host, behind Tasuke's own server (`tasuke server start`) on
# 127.0.0.1:18737; in the VM, tasuke-llm.service forwards 8737 to it, and
# the test copies the host server's key in as alice's.
#
# Deliberately not among the flake's checks: it needs a model server on the
# host, which the build sandbox can't reach. Start one with `port = 18737`
# under `[server]` in its config, then build .driver and run it outside the
# sandbox with private XDG_RUNTIME_DIR and TMPDIR, naming the server's key
# file and the model it serves (Gemma 4 26B-A4B if unnamed):
#   nix build .#tasuke-live.driver
#   TASUKE_LIVE_KEY=/path/to/its/api-key TASUKE_LIVE_MODEL=gemma-4-26b-a4b ./result/bin/nixos-test-driver
{
  pkgs,
  lib,
  tasuke,
  tasukeModule,
  aizomeModule,
}:
let
  catalog = builtins.fromTOML (builtins.readFile ../catalog/models.toml);
  files = pkgs.writeText "tasuke-catalog-files.json" (
    builtins.toJSON (builtins.mapAttrs (_: m: { inherit (m) name files; }) catalog.models)
  );
  # QEMU's user network reaches the host's loopback at 10.0.2.2.
  forward = pkgs.writeShellScriptBin "llama-server" ''
    exec ${lib.getExe pkgs.socat} TCP-LISTEN:8737,bind=127.0.0.1,fork,reuseaddr TCP:10.0.2.2:18737
  '';
in
pkgs.testers.runNixOSTest {
  name = "tasuke-live";
  globalTimeout = 1800;
  nodes.machine = {
    imports = [
      (import ./vm-desktop.nix {
        inherit
          pkgs
          tasuke
          tasukeModule
          aizomeModule
          ;
      })
    ];
    programs.tasuke.llamaCpp = lib.mkForce forward;
    virtualisation.restrictNetwork = false;
  };
  testScript = ''
    CATALOG = "${files}"
  ''
  + builtins.readFile ./vm-desktop.py
  + builtins.readFile ./vm-live-test.py;
}
