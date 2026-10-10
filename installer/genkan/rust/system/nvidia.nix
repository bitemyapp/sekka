# SPDX-License-Identifier: GPL-3.0-or-later
# NVIDIA's own driver, instead of nouveau, for machines with an NVIDIA GPU.
# The installer enables it when it finds one, with PRIME offload on laptops.
# Same package choice as the installation media, so an installation copies
# the driver from them: NVIDIA's packages are not in the public binary cache.
{ config, lib, ... }:
let
  cfg = config.calamares.nvidia;
  busId = lib.mkOption {
    type = lib.types.str;
    default = "";
    example = "PCI:1:0:0";
    description = "PCI bus ID in NixOS's decimal PCI:bus:device:function form.";
  };
  offload =
    cfg.prime.nvidiaBusId != "" && (cfg.prime.intelBusId != "" || cfg.prime.amdgpuBusId != "");
in
{
  options.calamares.nvidia = {
    enable = lib.mkEnableOption "NVIDIA's driver (latest release, open kernel modules)";
    prime = {
      nvidiaBusId = busId;
      intelBusId = busId;
      amdgpuBusId = busId;
    };
  };
  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        services.xserver.videoDrivers = [ "nvidia" ];
        hardware.graphics.enable = true;
        hardware.nvidia = {
          package = config.boot.kernelPackages.nvidiaPackages.latest;
          # NVIDIA's recommended modules from Turing on; the latest driver
          # supports no older GPU.
          open = true;
          modesetting.enable = true;
          # Saves video memory across suspend and hibernation.
          powerManagement.enable = true;
        };
      }
      (lib.mkIf offload {
        hardware.nvidia.prime = {
          offload.enable = true;
          offload.enableOffloadCmd = true;
          inherit (cfg.prime) nvidiaBusId intelBusId amdgpuBusId;
        };
        # Powers the NVIDIA GPU off while nothing uses it.
        hardware.nvidia.powerManagement.finegrained = true;
      })
    ]
  );
}
