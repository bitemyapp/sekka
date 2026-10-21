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
        }
        extra
      ];
    }).config;
  enabled = c: c.systemd.user.services ? aizome-ibus;
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
in
assert !enabled alone;
assert enabled coexist;
assert !enabled gnome;
assert !enabled xfce;
assert !enabled disabled;
assert !enabled custom;
assert coexist.environment.variables.GTK_IM_MODULE == "ibus";
assert coexist.environment.variables.QT_IM_MODULE == "ibus";
assert lib.hasInfix "GTK_IM_MODULE=wayland" coexist.environment.etc."xdg/aizome/environment".text;
assert lib.hasInfix "QT_IM_MODULE=wayland" coexist.environment.etc."xdg/aizome/environment".text;
assert coexist.systemd.user.services.aizome-ibus.partOf == [ "aizome-session.target" ];
assert coexist.services.displayManager.gdm.enable;
assert coexist.i18n.inputMethod.ibus.engines == [ ];
pkgs.runCommand "aizome-ibus-policy" { } ''
  echo 'Aizome alone, coexistence, GNOME, Xfce, disabled IBus and custom panel evaluated' > "$out"
''
