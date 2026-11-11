{ airplay }:
{ config, lib, ... }:
let
  cfg = config.programs.sekka-airplay;
in
{
  options.programs.sekka-airplay = {
    enable = lib.mkEnableOption "AirPlay screen mirroring with laptop audio";
    package = lib.mkOption {
      type = lib.types.package;
      default = airplay;
      description = "The Sekka AirPlay window and its sender.";
    };
    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Allow mDNS discovery and the sender's three UDP timing/audio ports. No TCP listener is opened.";
    };
  };
  config = lib.mkIf cfg.enable {
    environment.systemPackages = [ cfg.package ];
    # Standalone packages read the host policy as well, so a temporary
    # direct-Wi-Fi session cannot override openFirewall = false.
    environment.etc."sekka/airplay/open-firewall".text =
      if cfg.openFirewall then "true\n" else "false\n";
    # Interface-aware IPv6 discovery for the direct Wi-Fi link.
    services.avahi.enable = lib.mkDefault true;
    services.avahi.openFirewall = lib.mkDefault cfg.openFirewall;
    # The sender is a child of the window, never a system or login daemon.
    networking.firewall.allowedUDPPorts = lib.mkIf cfg.openFirewall [
      5353
      60000
      60001
      60002
    ];
  };
}
