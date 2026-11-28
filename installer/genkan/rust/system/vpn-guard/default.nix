# SPDX-License-Identifier: GPL-3.0-or-later
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.calamares.vpnGuard;
  enabled = cfg.enable && config.networking.networkmanager.enable;
  guard = pkgs.callPackage ./package.nix { };
  program = lib.getExe guard;
  settings = pkgs.writeTextFile {
    name = "sekka-vpn-guard.json";
    text = builtins.toJSON cfg.trustedLanPrefixes;
    checkPhase = ''${program} validate-config < "$target"'';
  };
in
{
  options.calamares.vpnGuard = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Protect host traffic in the address family a full-tunnel NetworkManager WireGuard profile does not carry.";
    };
    trustedLanPrefixes = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "2001:db8:1::40/128" ];
      description = "Additional trusted destination CIDRs reachable outside the tunnel, for example a LAN printer with a global IPv6 address.";
    };
  };
  config = lib.mkIf enabled {
    environment.systemPackages = [ guard ];
    environment.etc."sekka/vpn-guard.json".source = settings;
    networking.networkmanager.dispatcherScripts =
      map
        (type: {
          source = program;
          inherit type;
        })
        [
          "pre-up"
          "basic"
        ];
    systemd.services.sekka-vpn-guard = {
      description = "Apply the WireGuard address-family egress policy";
      wantedBy = [ "multi-user.target" ];
      # Arm the dispatcher before NetworkManager can activate its first profile.
      # Every later run comes from NetworkManager's own events, which cover
      # all the guard reads: the profiles NetworkManager has applied.
      before = [ "NetworkManager.service" ];
      # A changed executable or exception list is reconciled while the previous
      # nft table still protects the connection. Removal of the unit invokes stop.
      reloadIfChanged = true;
      restartTriggers = [ settings ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${program} start";
        ExecReload = "${program} reconcile";
        ExecStop = "${program} stop";
      };
    };
    # NixOS's nftables service flushes the whole ruleset when it is given
    # one of the administrator's own (`networking.nftables.ruleset` or
    # `rulesetFile`): as it starts, at each reload (a switch that changes
    # the ruleset reloads it) and as it stops. That deletes this table with
    # the rest while a tunnel stays connected, and no NetworkManager event
    # follows to put it back. The guard reconciles after each; it is a
    # no-op before the guard is armed, and its failure never fails the
    # firewall. The condition wraps the whole unit: one on `serviceConfig`
    # alone would declare an empty nftables.service on every system
    # without nftables.
    systemd.services.nftables = lib.mkIf config.networking.nftables.enable {
      serviceConfig = lib.genAttrs [
        "ExecStartPost"
        "ExecReload"
        "ExecStopPost"
      ] (_: lib.mkAfter [ "-${program} reconcile" ]);
    };
  };
}
