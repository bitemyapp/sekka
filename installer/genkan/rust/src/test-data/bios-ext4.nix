{ lib, ... }: {
  fileSystems."/" = {
    device = lib.mkForce "/dev/disk/by-uuid/11111111-2222-4333-8444-555555555555";
    fsType = "ext4";
    options = [ "noatime" ];
  };
  boot.loader.grub.gfxpayloadBios = "keep";
  fileSystems."/boot" = { device = lib.mkForce "/dev/disk/by-uuid/12345678-1234-4567-8901-123456789012"; fsType = "ext4"; options = [ "nosuid" "nodev" ]; };
  console.earlySetup = true;
  boot.initrd.luks.devices.cryptroot = { device = "/dev/disk/by-uuid/aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"; allowDiscards = true; };
  boot.initrd.luks.devices.cryptswap = { device = "/dev/disk/by-uuid/bbbbbbbb-cccc-4ddd-8eee-ffffffffffff"; allowDiscards = true; };
  # Swap partition matched to installed RAM, also used to resume from hibernation.
  swapDevices = lib.mkForce [ { device = "/dev/disk/by-uuid/66666666-7777-4888-9999-aaaaaaaaaaaa"; } ];
  boot.resumeDevice = "/dev/disk/by-uuid/66666666-7777-4888-9999-aaaaaaaaaaaa";
  calamares.zswap.enable = true;
}
