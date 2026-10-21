# Check the actual installer initrd, not a VM with the helper in its full store.
{ pkgs, installer }:
let
  helper = import ./iso-copytoram.nix { inherit pkgs; };
in
pkgs.runCommand "sekka-copytoram-initrd"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.zstd
      pkgs.proot
    ];
  }
  ''
    python3 ${./iso-copytoram-initrd-test.py} \
      ${installer.config.system.build.initialRamdisk}/initrd \
      ${helper}/bin/sekka-copy-iso-to-ram "$out"
  ''
