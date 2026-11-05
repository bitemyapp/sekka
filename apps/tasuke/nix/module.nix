# SPDX-License-Identifier: MIT OR Apache-2.0
# Tasuke's NixOS module. `self` is Tasuke's component (nix/components.nix),
# or null when imported without one, in which case the package is built here.
self:
{
  config,
  options,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.programs.tasuke;
in
{
  options.programs.tasuke = {
    enable = lib.mkEnableOption "Tasuke, the private assistant that runs on this machine";
    package = lib.mkOption {
      type = lib.types.package;
      default =
        if self == null then
          pkgs.callPackage ./package.nix { }
        else
          self.packages.${pkgs.stdenv.hostPlatform.system}.tasuke;
      defaultText = lib.literalExpression "sekka.packages.\${system}.tasuke";
      description = "The Tasuke package: the tasuke command, its root helper and its polkit action.";
    };
    llamaCpp = lib.mkOption {
      type = lib.types.package;
      default = pkgs.llama-cpp;
      defaultText = lib.literalExpression "pkgs.llama-cpp";
      example = lib.literalExpression "pkgs.llama-cpp.override { cudaSupport = true; }";
      description = ''
        The llama.cpp whose llama-server serves the models, in router mode.
        Build it for the machine's GPU: with CUDA for NVIDIA, Vulkan or ROCm
        for AMD.
      '';
    };
    checkup.enable = lib.mkEnableOption ''
      a weekly checkup: `tasuke checkup --notify` looks over the system
      without changing anything, and notifies only when something needs
      attention
    '';
    server.startOnLogin = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = ''
        Start the model server (the user unit tasuke-llm.service) when a
        person logs in. It loads a model only on first use and unloads it
        after `[server] idle_minutes` in ~/.config/tasuke/config.toml.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    # The command, the polkit action (share/polkit-1/actions) and the skills.
    environment.systemPackages = [ cfg.package ];

    # The helper runs through pkexec, which works only through NixOS's setuid
    # wrapper; recent NixOS has it only when something asks for it.
    security.polkit = {
      enable = true;
    }
    // lib.optionalAttrs (options.security.polkit ? enablePkexecWrapper) {
      enablePkexecWrapper = true;
    };

    # One llama-server in router mode, at low priority, so a game or a render
    # keeps the machine. `tasuke server exec` writes the presets for the
    # installed models and this hardware, then becomes llama-server with
    # the idle unload from config.toml.
    systemd.user.services.tasuke-llm = {
      description = "Tasuke's local model server";
      documentation = [ "https://github.com/bitemyapp/sekka/tree/main/apps/tasuke" ];
      wantedBy = lib.optional cfg.server.startOnLogin "default.target";
      # nvidia-smi and the rest of the system's tools, for hardware detection.
      path = [ "/run/current-system/sw" ];
      environment.CUDA_DEVICE_ORDER = "PCI_BUS_ID";
      serviceConfig = {
        # The key clients read, $XDG_RUNTIME_DIR/tasuke/api-key (0600).
        ExecStartPre = "${lib.getExe cfg.package} server key";
        ExecStart = "${lib.getExe cfg.package} server exec --llama-server ${lib.getExe' cfg.llamaCpp "llama-server"}";
        Restart = "on-failure";
        RestartSec = 5;
        Nice = 10;
        CPUWeight = 20;
        IOWeight = 20;
        OOMScoreAdjust = 500;
        # Children (one per loaded model) stop with the router.
        KillMode = "control-group";
        TimeoutStopSec = 30;
      };
    };

    # Read-only (Act off), at the lowest priority, after the model server.
    systemd.user.services.tasuke-checkup = lib.mkIf cfg.checkup.enable {
      description = "Tasuke's weekly checkup";
      wants = [ "tasuke-llm.service" ];
      after = [ "tasuke-llm.service" ];
      path = [ "/run/current-system/sw" ];
      serviceConfig = {
        Type = "oneshot";
        ExecStart = "${lib.getExe cfg.package} checkup --notify";
        Nice = 19;
        CPUWeight = 10;
        IOWeight = 10;
      };
    };
    systemd.user.timers.tasuke-checkup = lib.mkIf cfg.checkup.enable {
      description = "Tasuke's weekly checkup";
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = "Mon 10:00";
        Persistent = true;
        RandomizedDelaySec = "1h";
      };
    };
  };
}
