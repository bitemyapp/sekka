# A full-tunnel WireGuard connection that carries one address family must
# not leave the other on the ordinary network (bitemyapp/sekka#416), in
# disposable VMs:
#
#   guarded    a Sekka system built from the modules installed systems
#              import, with Aizome chosen, on a home network (eth1) that gives
#              it IPv4 by DHCP and a global IPv6 address by router
#              advertisement
#   isp        the home router: NAT for IPv4, routed IPv6
#   internet   a web host that answers with the address a request came from
#   vpn        a WireGuard server with an IPv4-only tunnel, an IPv6-only one
#              and one that carries both
#
# The VPN profiles are wg-quick files imported with nmcli, as Settings and
# Network import them. The test checks that sekka-vpn-guard, run by
# NetworkManager's dispatcher,
#
# - blocks IPv6 while the IPv4-only tunnel is up and IPv4 while the IPv6-only
#   one is, at once and not after a timeout, so that a site with both kinds
#   of address is reached through the tunnel without a wait;
# - preserves private and link-local access and permits a public LAN address
#   only when the configuration explicitly trusts it;
# - keeps a tunnel whose far end is in the blocked family working through a
#   fresh handshake, with NixOS's reverse-path filter on;
# - lifts the block when the tunnel goes down, and never sets it for a
#   tunnel that carries both families;
# - survives a live switch to an updated policy and is removed by a switch
#   to a generation with the option disabled.
#
# Component evidence only: wired interfaces, no real provider, and QEMU's
# user network (eth0) kept out of NetworkManager's hands so that eth1 is the
# only way out.
#
#     nix build .#checks.x86_64-linux.vpn-guard-vm -L
{
  pkgs,
  sekkaModule,
}:
let
  forwarding = {
    "net.ipv4.ip_forward" = 1;
    "net.ipv6.conf.all.forwarding" = 1;
  };
  address4 = address: {
    inherit address;
    prefixLength = 24;
  };
  address6 = address: {
    inherit address;
    prefixLength = 64;
  };
  client =
    { pkgs, lib, ... }:
    {
      imports = [ sekkaModule ];
      # What the installed flake passes the application catalog.
      _module.args = {
        applicationPkgs = pkgs;
        aiPackages = { };
        ompPackage = null;
      };
      calamares.desktops = [ "aizome" ];
      calamares.defaultDesktop = "aizome";
      calamares.applications = [ ];
      calamares.installUser = "demo";
      specialisation.trusted.configuration = {
        calamares.vpnGuard.trustedLanPrefixes = [ "2001:db8:1::1/128" ];
        # switch-to-configuration has stopped the old units by the time
        # activation runs, and has yet to start the new ones. Inspect the
        # kernel policy at this point while the tunnel stays connected.
        system.activationScripts.vpnGuardContinuity = {
          deps = [ "etc" ];
          text = ''
            if test -e /run/observe-vpn-switch; then
              if ${pkgs.nftables}/bin/nft list table inet sekka_vpn_guard >/dev/null &&
                 ! ${pkgs.curl}/bin/curl -g -6 -fsS --max-time 3 'http://[2001:db8:ffff::10]/' >/dev/null; then
                echo blocked > /run/vpn-during-switch
              else
                echo open > /run/vpn-during-switch
              fi
            fi
          '';
        };
      };
      specialisation.disabled.configuration.calamares.vpnGuard.enable = lib.mkForce false;
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        description = "Demo";
        password = "demo";
      };
      virtualisation = {
        vlans = lib.mkForce [ ];
        interfaces.eth1 = {
          vlan = 1;
          assignIP = false;
        };
        memorySize = 4096;
        cores = 2;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1280,yres=800"
        ];
      };
      hardware.graphics.enable = true;
      # Fixture only: QEMU's user network would be a second way out.
      networking.networkmanager.unmanaged = [ "interface-name:eth0" ];
      # Fixture tools for WireGuard and checking the installed kernel policy.
      environment.systemPackages = [
        pkgs.wireguard-tools
        pkgs.nftables
      ];
    };
in
pkgs.testers.runNixOSTest {
  name = "sekka-vpn-guard";
  globalTimeout = 1800;
  node.pkgsReadOnly = false;

  nodes.guarded = client;

  nodes.isp =
    { lib, ... }:
    {
      virtualisation.vlans = lib.mkForce [ ];
      virtualisation.interfaces.lan = {
        vlan = 1;
        assignIP = false;
      };
      virtualisation.interfaces.wan = {
        vlan = 2;
        assignIP = false;
      };
      networking.firewall.enable = false;
      networking.interfaces.lan = {
        ipv4.addresses = [ (address4 "192.168.1.1") ];
        ipv6.addresses = [
          (address6 "2001:db8:1::1")
          (address6 "fd42::1")
        ];
      };
      networking.interfaces.wan = {
        ipv4.addresses = [ (address4 "203.0.113.1") ];
        ipv6.addresses = [ (address6 "2001:db8:ffff::1") ];
        # A reachable service in the NAT64 prefix lets the client test
        # its IPv6 egress policy without depending on an actual translator.
        ipv6.routes = [
          {
            address = "64:ff9b::";
            prefixLength = 96;
            via = "2001:db8:ffff::10";
          }
        ];
      };
      boot.kernel.sysctl = forwarding;
      networking.nftables.enable = true;
      networking.nftables.ruleset = ''
        table ip nat {
          chain postrouting {
            type nat hook postrouting priority srcnat;
            oifname "wan" ip saddr 192.168.1.0/24 masquerade
          }
        }
      '';
      services.radvd = {
        enable = true;
        config = ''
          interface lan {
            AdvSendAdvert on;
            MinRtrAdvInterval 3;
            MaxRtrAdvInterval 10;
            prefix 2001:db8:1::/64 {
              AdvOnLink on;
              AdvAutonomous on;
            };
            prefix fd42::/64 {
              AdvOnLink on;
              AdvAutonomous on;
            };
          };
        '';
      };
      services.dnsmasq = {
        enable = true;
        resolveLocalQueries = false;
        settings = {
          port = 0;
          interface = "lan";
          bind-interfaces = true;
          dhcp-range = [ "192.168.1.100,192.168.1.150,12h" ];
          dhcp-option = [ "option:router,192.168.1.1" ];
        };
      };
    };

  nodes.internet =
    { lib, ... }:
    {
      virtualisation.vlans = lib.mkForce [ ];
      virtualisation.interfaces.eth1 = {
        vlan = 2;
        assignIP = false;
      };
      networking.interfaces.eth1 = {
        ipv4.addresses = [ (address4 "203.0.113.10") ];
        ipv6.addresses = [
          (address6 "2001:db8:ffff::10")
          {
            address = "64:ff9b::cb00:710a";
            prefixLength = 128;
          }
        ];
        # The way back to the home network's IPv6 prefix.
        ipv6.routes = [
          {
            address = "2001:db8:1::";
            prefixLength = 64;
            via = "2001:db8:ffff::1";
          }
        ];
      };
      networking.firewall.allowedTCPPorts = [ 80 ];
      services.nginx = {
        enable = true;
        virtualHosts.whoami = {
          default = true;
          listen = [
            {
              addr = "0.0.0.0";
              port = 80;
            }
            {
              addr = "[::]";
              port = 80;
            }
          ];
          locations."/".extraConfig = ''
            default_type text/plain;
            return 200 "$remote_addr\n";
          '';
        };
      };
    };

  nodes.vpn =
    { pkgs, lib, ... }:
    {
      virtualisation.vlans = lib.mkForce [ ];
      virtualisation.interfaces.eth1 = {
        vlan = 2;
        assignIP = false;
      };
      networking.firewall.enable = false;
      networking.interfaces.eth1 = {
        ipv4.addresses = [
          (address4 "203.0.113.20")
          (address4 "203.0.113.21")
        ];
        ipv6.addresses = [ (address6 "2001:db8:ffff::20") ];
      };
      boot.kernel.sysctl = forwarding;
      networking.nftables.enable = true;
      networking.nftables.ruleset = ''
        table ip nat {
          chain postrouting {
            type nat hook postrouting priority srcnat;
            oifname "eth1" ip saddr { 192.168.200.0/24, 192.168.201.0/24 } masquerade
          }
        }
        table ip6 nat {
          chain postrouting {
            type nat hook postrouting priority srcnat;
            oifname "eth1" ip6 saddr { fd00:200::/64, fd00:201::/64 } masquerade
          }
        }
      '';
      environment.systemPackages = [ pkgs.wireguard-tools ];
    };

  testScript = builtins.readFile ../tests/vm_start.py + builtins.readFile ./vpn-guard-vm-test.py;
}
