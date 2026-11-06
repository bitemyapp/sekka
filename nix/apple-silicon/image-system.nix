# Systems installed from Sekka's Mac image (calamares.appleSilicon.image):
# the image's root filesystem holds the system's closure and a registration
# of it (/nix-path-registration, nix/apple-silicon/image.nix), loaded into
# Nix's database once, at the first start, before anything uses Nix.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.calamares.appleSilicon;
in
{
  options.calamares.appleSilicon.image.enable = lib.mkEnableOption ''
    the first start of a system installed from Sekka's Mac image: its Nix
    store is registered once, from /nix-path-registration'';

  config = lib.mkIf (cfg.enable && cfg.image.enable) {
    systemd.services.sekka-image-first-boot = {
      description = "Register the image's Nix store";
      wantedBy = [ "sysinit.target" ];
      before = [
        "sysinit.target"
        "nix-daemon.socket"
        "nix-daemon.service"
      ];
      after = [ "local-fs.target" ];
      unitConfig = {
        DefaultDependencies = false;
        ConditionPathExists = "/nix-path-registration";
      };
      serviceConfig.Type = "oneshot";
      script = ''
        ${config.nix.package.out}/bin/nix-store --load-db < /nix-path-registration
        touch /etc/NIXOS
        ${config.nix.package.out}/bin/nix-env -p /nix/var/nix/profiles/system --set /run/current-system
        rm -f /nix-path-registration
      '';
    };
  };
}
