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
  # For the address family a full WireGuard tunnel does not carry,
  # NetworkManager's WireGuard device adds its own auto-default-route rule
  # pair with "unreachable" in place of the tunnel's table. NetworkManager
  # adds the pair as it activates the tunnel, before pre-up, and removes it
  # when the tunnel goes down or fails. See the patch.
  networkmanager = pkgs.networkmanager.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./networkmanager-block-uncarried-family.patch ];
    passthru = (old.passthru or { }) // {
      sekkaRefusesUncarriedFamily = true;
    };
  });
  # The patch's pair sits at 31800 and 31801. Trusted destinations come just
  # before it and go to the main table, as they would with no tunnel.
  trustedPriority = "31799";
  # One "-4 192.0.2.7/32" or "-6 2001:db8::/48" line per prefix. Python's
  # strict parse refuses a prefix with host bits set.
  trustedRules =
    pkgs.runCommand "sekka-vpn-guard-trusted"
      {
        nativeBuildInputs = [ pkgs.python3 ];
        prefixes = builtins.toJSON cfg.trustedLanPrefixes;
        passAsFile = [ "prefixes" ];
      }
      ''
        python3 -I -c '
        import ipaddress, json, sys
        for prefix in json.load(open(sys.argv[1])):
            net = ipaddress.ip_network(prefix)
            print(f"-{net.version} {net}")
        ' "$prefixesPath" > "$out"
      '';
  clearTrusted = ''
    for family in -4 -6; do
      while ip "$family" rule del priority ${trustedPriority} 2>/dev/null; do :; done
    done
  '';
  # NetworkManager leaves a connected tunnel's rules in place when it stops,
  # and a stock build that starts in its place does not know the patch's
  # pair, so it would never remove it.
  clearBlock = pkgs.writeShellScript "sekka-vpn-guard-clear" ''
    for family in -4 -6; do
      for priority in 31800 31801; do
        while ${pkgs.iproute2}/bin/ip "$family" rule del priority "$priority" 2>/dev/null; do :; done
      done
    done
  '';
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
      description = "Additional trusted destination CIDRs reachable outside the tunnel, for example a LAN printer with a global IPv6 address that is reached through the router.";
    };
  };
  config = lib.mkMerge [
    (lib.mkIf enabled {
      networking.networkmanager.package = lib.mkDefault networkmanager;
      # A reply to a tunnel's own IPv4 packets, in the family the tunnel does
      # not carry, gets the tunnel's mark back from conntrack. The kernel's
      # reverse-path check must look its source up with that mark, as
      # wg-quick has it, or it meets the "fwmark 0 unreachable" rule and the
      # reply is dropped. IPv6 has no such check in the kernel.
      boot.kernel.sysctl."net.ipv4.conf.all.src_valid_mark" = lib.mkDefault 1;
      assertions = [
        {
          assertion = config.networking.networkmanager.package.sekkaRefusesUncarriedFamily or false;
          message = ''
            calamares.vpnGuard needs Sekka's NetworkManager patch, and
            networking.networkmanager.package is set to a NetworkManager
            without it. Without the patch, a full WireGuard tunnel that carries
            one address family leaves the other on the physical network. Apply
            ${toString ./networkmanager-block-uncarried-family.patch} to that
            package, or set calamares.vpnGuard.enable = false.
          '';
        }
      ];
      systemd.services.sekka-vpn-guard-trusted = lib.mkIf (cfg.trustedLanPrefixes != [ ]) {
        description = "Reach trusted destinations outside a single-family WireGuard tunnel";
        wantedBy = [ "multi-user.target" ];
        before = [ "NetworkManager.service" ];
        path = [ pkgs.iproute2 ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        script = ''
          ${clearTrusted}
          while read -r family prefix; do
            ip "$family" rule add priority ${trustedPriority} to "$prefix" lookup main
          done < ${trustedRules}
        '';
        preStop = clearTrusted;
      };
    })
    # A switch to a generation with the guard off restarts NetworkManager
    # with the stock build while a tunnel can stay connected. Clear the
    # patch's pair before that build starts.
    (lib.mkIf (!cfg.enable && config.networking.networkmanager.enable) {
      systemd.services.NetworkManager.serviceConfig.ExecStartPre = [ "-${clearBlock}" ];
    })
  ];
}
