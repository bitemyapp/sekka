# Aizome's IBus policy across desktop choices: one check of the options, and
# one per system that its complete derivations evaluate. Each system is its own
# check so that CI's evaluation workers take them in parallel; together they
# were the slowest check to evaluate (38 s alone).
{ pkgs, inputs }:
let
  inherit (pkgs) lib;
  system =
    desktops: extra:
    (inputs.nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      modules = [
        inputs.calamares.nixosModules.desktops
        {
          calamares.desktops = desktops;
          calamares.defaultDesktop = builtins.head desktops;
          system.stateVersion = "26.11";
          boot.loader.grub.enable = false;
          fileSystems."/" = {
            device = "none";
            fsType = "tmpfs";
          };
        }
        extra
      ];
    }).config;
  enabled = c: c.systemd.user.services ? aizome-ibus;
  systemKeymap =
    c:
    lib.any (db: db.settings."desktop/ibus/general".use-system-keyboard-layout or false) (
      c.programs.dconf.profiles.aizome.databases or [ ]
    );
  alone = system [ "aizome" ] { };
  coexist = system [ "aizome" "gnome" "xfce" ] { };
  gnome = system [ "gnome" ] { };
  xfce = system [ "xfce" ] {
    i18n.inputMethod = {
      enable = true;
      type = "ibus";
    };
  };
  disabled = system [ "aizome" "gnome" ] { i18n.inputMethod.enable = lib.mkForce false; };
  custom = system [ "aizome" "gnome" ] { i18n.inputMethod.ibus.panel = "/custom/panel"; };
  withoutAizome = system [ "tatami" "xfce" "gnome" ] { };
  otherDesktops = map (desktop: system [ desktop ] { }) [
    "plasma"
    "hyprland"
    "tatami"
  ];
  # Names are fixed here, so listing the checks evaluates no system.
  configurations = {
    inherit
      alone
      coexist
      gnome
      xfce
      disabled
      custom
      ;
    without-aizome = withoutAizome;
    plasma = builtins.elemAt otherDesktops 0;
    hyprland = builtins.elemAt otherDesktops 1;
    tatami = builtins.elemAt otherDesktops 2;
  };
  options =
    assert !enabled alone;
    assert enabled coexist;
    assert !enabled gnome;
    assert !enabled xfce;
    assert !enabled disabled;
    assert !enabled custom;
    assert !enabled withoutAizome;
    assert !(gnome.environment.etc ? "xdg/aizome/environment");
    assert !(xfce.environment.etc ? "xdg/aizome/environment");
    assert !(withoutAizome.environment.etc ? "xdg/aizome/environment");
    assert systemKeymap coexist;
    assert builtins.all (c: !systemKeymap c) (
      [
        alone
        gnome
        xfce
        disabled
        custom
        withoutAizome
      ]
      ++ otherDesktops
    );
    assert builtins.all (c: !(c.environment.etc ? "xdg/aizome/environment")) otherDesktops;
    # Aizome must leave other desktops' GTK policy alone, including newer
    # NixOS defaults that no longer export a global GTK_IM_MODULE.
    assert
      (coexist.environment.variables.GTK_IM_MODULE or null)
      == (gnome.environment.variables.GTK_IM_MODULE or null);
    assert coexist.environment.variables.QT_IM_MODULE == "ibus";
    assert lib.hasInfix "GTK_IM_MODULE=wayland" coexist.environment.etc."xdg/aizome/environment".text;
    assert lib.hasInfix "QT_IM_MODULE=wayland" coexist.environment.etc."xdg/aizome/environment".text;
    assert coexist.systemd.user.services.aizome-ibus.partOf == [ "aizome-session.target" ];
    assert coexist.services.displayManager.gdm.enable;
    assert coexist.i18n.inputMethod.ibus.engines == [ ];
    pkgs.runCommand "aizome-ibus-policy" { } ''
      echo 'IBus policy for each desktop, Aizome coexistence, disabled IBus, custom panel and Tatami/Xfce/GNOME without Aizome' > "$out"
    '';
in
{
  aizome-ibus-policy = options;
}
# Option-level assertions do not force lazy /etc entries or their sources.
# Evaluate complete derivations in every case without building the systems.
// pkgs.lib.mapAttrs' (
  name: c:
  pkgs.lib.nameValuePair "aizome-ibus-policy-${name}" (
    assert c.system.build.etc.drvPath != "" && c.system.build.toplevel.drvPath != "";
    pkgs.runCommand "aizome-ibus-policy-${name}" { } ''
      echo 'Complete etc/system derivations evaluated for ${name}' > "$out"
    ''
  )
) configurations
