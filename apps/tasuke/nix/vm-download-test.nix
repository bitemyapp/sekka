# SPDX-License-Identifier: MIT OR Apache-2.0
# The window's first run downloads a real model from Hugging Face, as a
# person with an 8 GB laptop GPU would (vm-desktop.nix's machine): a
# stand-in nvidia-smi reports the card, first run recommends what fits,
# and the test presses Download, Pause and Resume, then checks each file
# against the catalog's size and sha256 and that the model is in use.
#
# Deliberately not among the flake's checks: it needs the internet, which
# the build sandbox doesn't have, and several gigabytes. Build .driver and
# run it outside the sandbox with private XDG_RUNTIME_DIR and TMPDIR:
#   nix build .#tasuke-download.driver && ./result/bin/nixos-test-driver
{
  pkgs,
  tasuke,
  tasukeModule,
  aizomeModule,
}:
let
  catalog = builtins.fromTOML (builtins.readFile ../catalog/models.toml);
  files = pkgs.writeText "tasuke-catalog-files.json" (
    builtins.toJSON (builtins.mapAttrs (_: m: { inherit (m) name files; }) catalog.models)
  );
  # nvidia-smi --query-gpu=index,name,memory.total,memory.used,memory.free,
  # pci.bus_id,compute_cap --format=csv,noheader,nounits on such a laptop.
  nvidiaSmi = pkgs.writeShellScriptBin "nvidia-smi" ''
    echo "0, NVIDIA GeForce RTX 4060 Laptop GPU, 8188, 300, 7888, 00000000:01:00.0, 8.9"
  '';
in
pkgs.testers.runNixOSTest {
  name = "tasuke-download";
  globalTimeout = 3600;
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
    environment.systemPackages = [ nvidiaSmi ];
    virtualisation = {
      restrictNetwork = false;
      diskSize = 16384;
    };
  };
  testScript = ''
    CATALOG = "${files}"
  ''
  + builtins.readFile ./vm-desktop.py
  + builtins.readFile ./vm-download-test.py;
}
