# External monitors' brightness through the Aizome module, in a disposable
# VM, with the real kernel's USB HID, hidraw, DRM and udev:
#
# - The module loads i2c-dev and installs its udev rules ahead of
#   systemd's uaccess; an I2C bus that isn't a display adapter's (i2c-stub)
#   stays root's.
# - The VM's display shows an EDID like a Studio Display's
#   (drm.edid_firmware), and a USB gadget on dummy_hcd stands in for its
#   USB side (vm-monitor-brightness-studio-display.py): interfaces 0-7, a
#   HID at 5 and the brightness HID at 7. Whoever is at the computer
#   (alice, logged in on tty1) gets interface 7 and not 5; someone else
#   (bob) gets neither.
# - aizome-brightness, as alice, finds the display on its connector, reads
#   its level over HID, sets it, and remembers it.
#
# Component evidence: no real Studio Display (its Thunderbolt link, its
# firmware), DDC/CI monitor or desktop session is involved.
{
  pkgs,
  self,
  package ? null,
}:
let
  fake = ./vm-monitor-brightness-studio-display.py;
  edid = pkgs.runCommand "aizome-test-studio-display-edid" { } ''
    mkdir -p $out/lib/firmware/edid
    ${pkgs.python3}/bin/python3 -I ${fake} edid > $out/lib/firmware/edid/aizome-studio-display.bin
  '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-monitor-brightness";
  globalTimeout = 600;
  nodes.machine =
    { lib, ... }:
    {
      imports = [ self.nixosModules.default ];
      programs.aizome.enable = true;
      programs.aizome.package = lib.mkIf (package != null) package;
      users.users.alice = {
        isNormalUser = true;
        uid = 1000;
      };
      users.users.bob = {
        isNormalUser = true;
        uid = 1001;
      };
      # At the computer: a session on seat0, active on tty1.
      services.getty.autologinUser = "alice";
      # virtio_gpu reads its EDID in the initrd, so the file is there too.
      hardware.firmware = [ edid ];
      boot.initrd.extraFirmwarePaths = [ "edid/aizome-studio-display.bin" ];
      boot.kernelParams = [ "drm.edid_firmware=edid/aizome-studio-display.bin" ];
      environment.systemPackages = [ pkgs.acl ];
      systemd.services.fake-studio-display = {
        description = "A made-up Studio Display's USB side";
        path = [
          pkgs.kmod
          pkgs.util-linux
        ];
        serviceConfig.ExecStart = "${pkgs.python3}/bin/python3 -I ${fake} serve";
      };
    };
  testScript = builtins.readFile ./vm-monitor-brightness-test.py;
}
