# Bluetooth LE privacy as Aizome chooses it (aizome-sys's
# bluetooth::privacy), on BlueZ's virtual controllers: on for a computer
# with nothing paired over LE, off for one with a device paired over LE
# while it was off, and as Settings asks through pkexec once polkit says yes
# (a rule standing in for an administrator), from bluetoothd's next start,
# which only the restart asked for brings. Without polkit's yes, refused.
{
  pkgs,
  self,
}:
let
  btvirt = import ./btvirt.nix { inherit pkgs; };
in
pkgs.testers.runNixOSTest {
  name = "aizome-bluetooth-privacy";
  globalTimeout = 600;
  nodes.machine = {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
    };
    users.users.bob = {
      isNormalUser = true;
      uid = 1001;
    };
    # Instead of alice's password dialog: polkit says yes to her alone.
    security.polkit.extraConfig = ''
      polkit.addRule(function (action, subject) {
        if (action.id == "org.aizome.Settei.bluetooth" && subject.user == "alice") {
          return polkit.Result.YES;
        }
      });
    '';
    # The script restarts bluetoothd more often than systemd's start limit
    # (five in ten seconds) allows.
    systemd.services.bluetooth.startLimitIntervalSec = 0;
    boot.kernelModules = [ "hci_vhci" ];
    systemd.services.aizome-test-btvirt = {
      description = "Virtual Bluetooth controllers";
      before = [ "bluetooth.service" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig.ExecStart = "${btvirt}/bin/btvirt -l2";
    };
    virtualisation.memorySize = 2048;
  };
  testScript = builtins.readFile ./vm-bluetooth-privacy-test.py;
}
