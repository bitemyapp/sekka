# Real UDisks formatting/mount policy on a disposable VM disk. This does
# not exercise the Tansu window or qualify an installer image.
{ pkgs, self }:
let
  evaluate =
    extra:
    (import (pkgs.path + "/nixos/lib/eval-config.nix") {
      system = pkgs.stdenv.hostPlatform.system;
      modules = [
        self.nixosModules.default
        { programs.aizome.enable = true; }
        extra
      ];
    }).config.services.udisks2.settings;
  defaults = evaluate { };
  overridden = evaluate {
    services.udisks2.settings."mount_options.conf".defaults.btrfs_defaults = "compress=no";
  };
in
assert defaults."mount_options.conf".defaults.btrfs_defaults == "compress=zstd:1";
assert overridden."mount_options.conf".defaults.btrfs_defaults == "compress=no";
pkgs.testers.runNixOSTest {
  name = "aizome-tansu-compression";
  globalTimeout = 240;
  nodes.machine = {
    # Consume the actual desktop module's evaluated service configuration,
    # without building a graphical session for this storage policy test.
    services.udisks2 = {
      enable = true;
      settings = defaults;
    };
    environment.systemPackages = [
      pkgs.btrfs-progs
      pkgs.compsize
      pkgs.python3
    ];
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
    };
    security.polkit.extraConfig = ''
      polkit.addRule(function(action, subject) {
        if (subject.user == "alice" && action.id.indexOf("org.freedesktop.udisks2.") == 0) {
          return polkit.Result.YES;
        }
      });
    '';
    virtualisation = {
      memorySize = 1024;
      cores = 2;
      emptyDiskImages = [ 1024 ];
    };
  };
  testScript = builtins.readFile ./vm-tansu-compression-test.py;
}
