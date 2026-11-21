# SPDX-License-Identifier: GPL-3.0-or-later
# A full-tunnel WireGuard connection carries the address families its
# AllowedIPs name, and NetworkManager routes only those into it. With an
# IPv4-only tunnel up, IPv6 still leaves by the Wi-Fi or Ethernet connection,
# and a site that has both kinds of address is reached that way first: it sees
# the home address the tunnel was meant to hide. The same holds the other way
# round (bitemyapp/sekka#416).
#
# sekka-vpn-guard makes the family the tunnel does not carry unreachable for
# as long as the tunnel is up, and leaves the home network reachable in it.
# NetworkManager's dispatcher runs it at every network change. The service
# runs it once when a configuration is switched to, for a tunnel that is
# already up, and lifts the guard when this module is turned off
# (DEVELOPERS.md, "VPN address families").
{
  config,
  lib,
  pkgs,
  ...
}:
let
  guard = pkgs.callPackage ./package.nix { };
  program = lib.getExe guard;
in
{
  options.calamares.vpnGuard.enable = lib.mkOption {
    type = lib.types.bool;
    default = true;
    description = "Block the address family a full-tunnel WireGuard connection does not carry, so that it cannot leave outside the tunnel.";
  };
  config = lib.mkIf (config.calamares.vpnGuard.enable && config.networking.networkmanager.enable) {
    # For `sekka-vpn-guard status`, which says what is blocked and why.
    environment.systemPackages = [ guard ];
    networking.networkmanager.dispatcherScripts = [
      {
        source = program;
        type = "basic";
      }
    ];
    systemd.services.sekka-vpn-guard = {
      description = "Block the address family a full-tunnel VPN does not carry";
      wantedBy = [ "multi-user.target" ];
      after = [ "NetworkManager.service" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        ExecStart = "${program} reconcile";
        ExecStop = "${program} clear";
      };
    };
  };
}
