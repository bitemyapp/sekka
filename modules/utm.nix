# UTM's QEMU backend on Apple silicon: native CPU virtualization and VirGL.
# Imported by the ARM media and its generated installed flake.
{
  lib,
  pkgs,
  modulesPath,
  ...
}:
{
  imports = [ (modulesPath + "/profiles/qemu-guest.nix") ];

  assertions = [
    {
      assertion = pkgs.stdenv.hostPlatform.system == "aarch64-linux";
      message = "Sekka's UTM profile requires an aarch64-linux guest.";
    }
  ];
  services.qemuGuest.enable = true;
  services.spice-vdagentd.enable = true;
  hardware.graphics.enable = true;
  boot.initrd.availableKernelModules = [
    "virtio_pci"
    "virtio_blk"
    "virtio_scsi"
    "virtio_gpu"
  ];
  boot.kernelModules = [ "virtio_gpu" ];
  # UTM 5's Venus path cannot yet render Linux desktops correctly. GTK 4's
  # default Vulkan renderer must use GL, while Mesa chooses VirGL for VirtIO.
  environment.sessionVariables.GSK_RENDERER = lib.mkDefault "gl";
  # VM time is corrected by the host agent after a host sleep/resume.
  services.timesyncd.enable = lib.mkDefault true;
}
