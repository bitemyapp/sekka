# Evaluate who logs in by themselves on an installed Sekka (#272): the
# choice Settei proposes and Yukimi writes, and what each login screen gets
# from it. No running system or boot configuration is changed.
{ pkgs, inputs }:
let
  inherit (pkgs) lib;
  installed =
    extra:
    (inputs.nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      modules = [
        inputs.calamares.nixosModules.desktops
        {
          calamares.desktops = [ "aizome" ];
          calamares.defaultDesktop = "aizome";
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
  nobody = installed { };
  kara = installed { calamares.autoLogin = "kara"; };
  existing = {
    services.displayManager.autoLogin = {
      enable = true;
      user = "adama";
    };
  };
  inherited = installed existing;
  turnedOff = installed {
    imports = [ existing ];
    calamares.autoLogin = "";
  };
  changed = installed {
    imports = [ existing ];
    calamares.autoLogin = "kara";
  };
  forced = installed {
    calamares.autoLogin = "kara";
    services.displayManager.autoLogin = {
      enable = lib.mkForce false;
      user = lib.mkForce null;
    };
  };
  gnome = installed {
    calamares.desktops = lib.mkForce [
      "aizome"
      "gnome"
    ];
    calamares.loginScreen = "gdm";
    calamares.autoLogin = "kara";
  };
  facts =
    c:
    builtins.fromJSON (builtins.unsafeDiscardStringContext c.environment.etc."yukimi/system.json".text);
  choice = c: lib.filter (choice: choice.setting == "calamares.autoLogin") (facts c).choices;
  offered =
    value: c:
    let
      choices = choice c;
    in
    builtins.length choices == 1
    && (builtins.head choices).people
    && !(builtins.head choices).many
    && !(builtins.head choices).boolean
    && (builtins.head choices).value == value
    && (builtins.head choices).options == [ ];
in
# Offered either way, as a person's name or empty for no one.
assert offered "" nobody;
assert offered "kara" kara;
assert offered "adama" inherited;
assert inherited.services.greetd.settings.initial_session.user == "adama";
assert offered "" turnedOff;
assert !turnedOff.services.displayManager.autoLogin.enable;
assert !(turnedOff.services.greetd.settings ? initial_session);
assert offered "kara" changed;
assert changed.services.greetd.settings.initial_session.user == "kara";
# The displayed value is the effective setting, including forced policy.
assert offered "" forced;
assert !forced.services.displayManager.autoLogin.enable;
assert !nobody.services.displayManager.autoLogin.enable;
assert
  kara.services.displayManager.autoLogin.enable
  && kara.services.displayManager.autoLogin.user == "kara";
# Aizome's login screen starts that person's session through greetd at boot.
assert !(nobody.services.greetd.settings ? initial_session);
assert kara.services.greetd.settings.initial_session.user == "kara";
assert lib.hasInfix "aizome-greeter --session"
  kara.services.greetd.settings.initial_session.command;
# GDM gets it from the same option.
assert
  gnome.services.displayManager.gdm.enable && gnome.services.displayManager.autoLogin.user == "kara";
# Whole systems with the choice made, under greetd and GDM. The others only
# change option values, checked above, so they aren't evaluated whole.
assert builtins.all (c: c.system.build.toplevel.drvPath != "" && c.system.build.etc.drvPath != "") [
  kara
  gnome
];
pkgs.writeText "auto-login-choice.json" (
  builtins.toJSON {
    nobody = choice nobody;
    kara = choice kara;
  }
)
