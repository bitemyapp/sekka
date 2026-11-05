# SPDX-License-Identifier: GPL-3.0-or-later
# NetworkManager's saved connections, passwords and all, kept in a small
# LUKS2 image whose only key is sealed to the TPM and bound over its keyfile
# directory before it starts (network-store/src/main.rs; DEVELOPERS.md,
# "Saved network connections"). NetworkManager itself is unchanged, so saved
# networks connect at boot, before anyone logs in, in every desktop.
# Installed systems only: the live session doesn't import this module.
{
  config,
  lib,
  pkgs,
  utils,
  ...
}:
let
  cfg = config.calamares.networkStore;
  nm = config.networking.networkmanager;
  keyfiles = nm.settings.keyfile.path or "/etc/NetworkManager/system-connections";
  state = "/var/lib/sekka/network-store";
  image = "${state}/store.luks";
  store = pkgs.callPackage ./network-store/package.nix { };
  run =
    command:
    lib.escapeShellArgs [
      (lib.getExe store)
      command
      "--state"
      state
      "--keyfiles"
      keyfiles
      "--tpm2-setup"
      "${config.systemd.package}/lib/systemd/systemd-tpm2-setup"
    ];
  # cryptsetup formats; systemd's own cryptsetup and cryptenroll carry the
  # TPM2 token plugin.
  tools = [
    pkgs.cryptsetup
    config.systemd.package
    pkgs.e2fsprogs
    pkgs.util-linux
  ];
in
{
  options.calamares.networkStore.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = ''
      Keep NetworkManager's saved connections, and their passwords, in a
      store sealed to this computer's TPM. Turned off, they're moved back
      out at the next boot.
    '';
  };

  config = lib.mkIf nm.enable (
    lib.mkMerge [
      {
        # Every generation says it can open a store; one is only made once no
        # bootable generation would boot without the saved networks.
        system.systemBuilderCommands = ''
          mkdir -p $out/sekka
          touch $out/sekka/network-store-v1
        '';
        assertions = [
          {
            assertion = !config.system.etc.overlay.enable;
            message = "calamares.networkStore mounts over ${keyfiles}, which an /etc overlay drops at a switch.";
          }
        ];
        # Not a disk for file managers to offer.
        services.udev.extraRules = ''
          ENV{DM_NAME}=="sekka-network-store*", ENV{UDISKS_IGNORE}="1"
          SUBSYSTEM=="block", KERNEL=="loop*", ATTR{loop/backing_file}=="${state}/*", ENV{UDISKS_IGNORE}="1"
        '';
      }
      (lib.mkIf cfg.enable {
        systemd.services.sekka-network-store = {
          description = "Open the TPM-sealed store of saved network connections";
          wantedBy = [ "NetworkManager.service" ];
          before = [ "NetworkManager.service" ];
          path = tools;
          # A switch must never close the store under NetworkManager.
          restartIfChanged = false;
          stopIfChanged = false;
          unitConfig = {
            ConditionPathExists = image;
            RequiresMountsFor = state;
            X-StopOnRemoval = false;
          };
          serviceConfig = {
            Type = "oneshot";
            RemainAfterExit = true;
            # NetworkManager waits for this, so it's bounded.
            TimeoutStartSec = "30s";
            ExecStart = run "open";
            ExecStop = run "stop";
          };
        };
        # At shutdown, NetworkManager stops before the store is unmounted.
        systemd.services.NetworkManager.after = [ "${utils.escapeSystemdPath keyfiles}.mount" ];
        systemd.services.sekka-network-store-create = {
          description = "Make the TPM-sealed store of saved network connections";
          wantedBy = [ "multi-user.target" ];
          # Only after: wanting tpm2.target would make a machine whose TPM has
          # no driver wait 90 s for its device. This isn't on NetworkManager's
          # way, so it may wait.
          after = [
            "tpm2.target"
            "sekka-network-store.service"
          ];
          path = tools;
          unitConfig = {
            # A TPM with its driver loaded. (ConditionSecurity=tpm2 also wants
            # the firmware to report one over EFI, and BIOS machines don't.)
            ConditionPathExistsGlob = "/sys/class/tpmrm/tpmrm*";
            RequiresMountsFor = state;
          };
          serviceConfig = {
            Type = "oneshot";
            TimeoutStartSec = "5min";
            ExecStart = run "create";
          };
        };
        systemd.paths.sekka-network-store-skeleton = {
          description = "Watch saved network connections for their secret-less copies";
          wantedBy = [ "multi-user.target" ];
          # Watch the store, once it's bound. A path unit is otherwise before
          # paths.target, so before basic.target, which the store's unit
          # follows: systemd would break that cycle by dropping a start job.
          after = [ "sekka-network-store.service" ];
          before = [ "shutdown.target" ];
          conflicts = [ "shutdown.target" ];
          unitConfig.DefaultDependencies = false;
          pathConfig.PathChanged = keyfiles;
        };
        systemd.services.sekka-network-store-skeleton = {
          description = "Refresh the secret-less copies of saved network connections";
          path = tools;
          serviceConfig = {
            Type = "oneshot";
            ExecStart = run "skeleton";
          };
        };
      })
      (lib.mkIf (!cfg.enable) {
        systemd.services.sekka-network-store-export = {
          description = "Move saved network connections out of the TPM-sealed store";
          wantedBy = [ "NetworkManager.service" ];
          before = [ "NetworkManager.service" ];
          path = tools;
          unitConfig = {
            ConditionPathExists = image;
            RequiresMountsFor = state;
          };
          serviceConfig = {
            Type = "oneshot";
            TimeoutStartSec = "30s";
            ExecStart = run "export";
          };
        };
      })
    ]
  );
}
