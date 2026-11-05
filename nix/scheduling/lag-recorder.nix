# SPDX-License-Identifier: GPL-3.0-or-later
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.sekka-lag-recorder;
  recorder = import ./lag-recorder-package.nix { inherit pkgs; };
in
{
  options.services.sekka-lag-recorder.enable = lib.mkEnableOption "private, bounded local lag recording";

  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ recorder ];
    systemd.user.services.sekka-lag-recorder = {
      description = "Record local pressure and scheduling evidence during desktop lag";
      wantedBy = [ "default.target" ];
      serviceConfig = {
        ExecStart = "${recorder}/bin/sekka-lag-recorder";
        Slice = "background.slice";
        CPUWeight = 20;
        IOWeight = 20;
        MemoryMax = "128M";
        Restart = "on-failure";
        RestartSec = 5;
        UMask = "0077";
        NoNewPrivileges = true;
      };
    };
  };
}
