# Systems installed from Sekka's Mac image (calamares.appleSilicon.image):
# the image's root filesystem holds the system's closure and a registration
# of it (/nix-path-registration, nix/apple-silicon/image.nix), loaded into
# Nix's database once, at the first start, before anything uses Nix.
#
# With first-boot setup, the first rebuild after it is built from what the
# image holds (image.nix, rebuildInputs). glibc's locales are compiled from
# i18n.supportedLocales in its order, and NixOS's default puts the default
# locale third, so all but two of setup's languages would need a new
# glibc-locales, compiled on the Mac with a toolchain the image doesn't hold.
# Here the order doesn't depend on the choice: every language setup offers
# (i18n.extraLocales) is already in the list, and the image's glibc-locales
# serves them all.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.calamares.appleSilicon;
  i18n = config.i18n;
  # As nixos/modules/config/i18n.nix spells locales.
  sanitize = lib.replaceStrings [ "utf8" "utf-8" "UTF8" ] [ "UTF-8" "UTF-8" "UTF-8" ];
  settings = lib.mapAttrsToList (
    role: lang: "${sanitize lang}/${i18n.localeCharsets.${role} or "UTF-8"}"
  ) (lib.filterAttrs (_: lang: lang != "C") (removeAttrs i18n.extraLocaleSettings [ "LANGUAGE" ]));
in
{
  options.calamares.appleSilicon.image.enable = lib.mkEnableOption ''
    the first start of a system installed from Sekka's Mac image: its Nix
    store is registered once, from /nix-path-registration'';

  config = lib.mkIf (cfg.enable && cfg.image.enable) (
    lib.mkMerge [
      {
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
      }

      # Between setup and the first update, the running system is still the
      # image's, which configures no host name, so hostnamed keeps setup's
      # choice in /etc/hostname, and Settings, which treats a host name
      # outside the store as the system's own, offers to rename the computer.
      # But setup.nix now declares the name, and the flake names its only
      # system after it: after a rename, nixos-rebuild finds no system for
      # the new name, and Yukimi builds the declared one, quietly putting the
      # old name back. So once setup.nix exists, the name is kept in the
      # store, as a configured one is, until the update that builds it
      # replaces /etc/hostname with its own.
      (lib.mkIf ((config.calamares.firstBootSetup.enable or false) && config.networking.hostName == "") {
        systemd.paths.sekka-setup-hostname = {
          description = "Keep the host name first-boot setup chose";
          wantedBy = [ "multi-user.target" ];
          pathConfig.PathExists = "/etc/nixos/setup.nix";
        };
        systemd.services.sekka-setup-hostname = {
          description = "Keep the host name first-boot setup chose until the update that builds it";
          path = [
            config.nix.package.out
            pkgs.coreutils
          ];
          serviceConfig = {
            Type = "oneshot";
            RemainAfterExit = true;
          };
          script = ''
            if [ -L /etc/hostname ] || [ ! -s /etc/hostname ]; then
              exit 0
            fi
            kept=$(nix-store --add /etc/hostname)
            ln -sfn "$kept" /nix/var/nix/gcroots/sekka-setup-hostname
            ln -sfn "$kept" /etc/.hostname.sekka
            mv -T /etc/.hostname.sekka /etc/hostname
            echo "the host name $(cat /etc/hostname) is kept in $kept until the next update builds it"
          '';
        };
      })

      (lib.mkIf (config.calamares.firstBootSetup.enable or false) {
        i18n.supportedLocales = lib.mkDefault (
          lib.unique (
            [
              "C.UTF-8/UTF-8"
              "en_US.UTF-8/UTF-8"
            ]
            ++ map sanitize (lib.toList i18n.extraLocales)
            ++ lib.optional (i18n.defaultLocale != "C") "${sanitize i18n.defaultLocale}/${i18n.defaultCharset}"
            ++ settings
          )
        );
      })
    ]
  );
}
