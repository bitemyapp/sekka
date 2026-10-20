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
  # virtio-ramfb-gl leaves its UEFI framebuffer registered as a second display.
  # Compositors otherwise place windows on an invisible 800x600 Unknown-1
  # output beside Virtual-1. Keep the accelerated VirtIO output enabled.
  boot.kernelParams = [ "video=Unknown-1:d" ];
  # UTM 5's Venus path cannot yet render Linux desktops correctly. GTK 4's
  # default Vulkan renderer must use GL, while Mesa chooses VirGL for VirtIO.
  environment.sessionVariables.GSK_RENDERER = lib.mkDefault "gl";
  # The service below owns startup in every desktop, including those that do
  # not honor X-systemd-skip in the package's autostart file.
  environment.etc."xdg/autostart/spice-vdagent.desktop".text = ''
    [Desktop Entry]
    Type=Application
    Name=SPICE desktop integration
    Hidden=true
  '';
  # Plasma's systemd session skips the package's XDG autostart entry. The
  # system daemon alone cannot bridge the user's clipboard to SPICE.
  systemd.user.services.spice-vdagent = {
    description = "SPICE desktop integration";
    wantedBy = [ "graphical-session.target" ];
    after = [ "graphical-session.target" ];
    partOf = [ "graphical-session.target" ];
    unitConfig.ConditionEnvironment = "DISPLAY";
    serviceConfig = {
      ExecStart = "${pkgs.spice-vdagent}/bin/spice-vdagent -x";
      Restart = "on-failure";
      RestartSec = 2;
    };
  };
  # VM time is corrected by the host agent after a host sleep/resume.
  services.timesyncd.enable = lib.mkDefault true;
}
