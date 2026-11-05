# The shell terminals open in. fish, set up as Omarchy sets it up, wherever
# Aizome is installed, whatever applications were chosen; bash with Tatami or
# another desktop alone (Omarchy's own fish is opt-in), unless the system chose
# the catalog's former "fish" or turned calamares.fish on, as Yukimi used to
# offer. Yukimi now offers calamares.terminalShell: fish, bash, zsh or
# nushell, which takes precedence over calamares.fish.
# Evaluation only; checks.fish logs in to a system with fish, and
# checks.terminal-shells opens each shell in each terminal.
{
  pkgs,
  inputs,
  applicationCatalog,
}:
let
  inherit (pkgs) lib;
  installed =
    settings:
    (inputs.nixpkgs.lib.nixosSystem {
      system = pkgs.stdenv.hostPlatform.system;
      inherit (applicationCatalog) specialArgs;
      modules = [
        inputs.calamares.nixosModules.default
        {
          calamares.installUser = "alice";
          calamares.applications = lib.mkDefault [ ];
          users.users.alice.isNormalUser = true;
          system.stateVersion = "26.11";
          boot.loader.grub.enable = false;
          fileSystems."/" = {
            device = "none";
            fsType = "tmpfs";
          };
        }
        settings
      ];
    }).config;
  desktops = list: {
    calamares.desktops = list;
    calamares.defaultDesktop = builtins.head list;
  };
  aizome = installed (desktops [ "aizome" ]);
  together = installed (desktops [
    "tatami"
    "aizome"
  ]);
  tatami = installed (desktops [ "tatami" ]);
  plasma = installed (desktops [ "plasma" ]);
  # A Tatami system that chose fish among the applications before it came
  # with Aizome keeps it.
  chosen = installed {
    imports = [ (desktops [ "tatami" ]) ];
    calamares.applications = [
      "fish"
      "firefox"
    ];
  };
  declined = installed {
    imports = [ (desktops [ "aizome" ]) ];
    calamares.fish.enable = false;
  };
  declinedWithFish = installed (
    { pkgs, ... }:
    {
      imports = [ (desktops [ "aizome" ]) ];
      calamares.fish.enable = false;
      # Fish can still be useful for scripts without being the default shell.
      environment.systemPackages = [ pkgs.fish ];
      calamares.applications = [ "fish" ];
    }
  );
  zsh = installed {
    imports = [ (desktops [ "tatami" ]) ];
    calamares.terminalShell = "zsh";
  };
  nushell = installed {
    imports = [ (desktops [ "aizome" ]) ];
    calamares.terminalShell = "nushell";
  };
  # What Yukimi wrote before it offered zsh and nushell stays, beside the
  # choice it writes now, which wins.
  reconsidered = installed {
    imports = [ (desktops [ "aizome" ]) ];
    calamares.fish.enable = false;
    calamares.terminalShell = "fish";
  };
  replaced = installed {
    imports = [ (desktops [ "tatami" ]) ];
    calamares.fish.enable = true;
    calamares.terminalShell = "zsh";
  };
  hotaruShell = c: c.environment.etc."xdg/aizome/hotaru-shell".text;
  init = c: builtins.unsafeDiscardStringContext c.programs.bash.interactiveShellInit;
  handsOver =
    c: exe: comm:
    lib.hasInfix (builtins.unsafeDiscardStringContext "!= \"${comm}\"") (init c)
    && lib.hasInfix (builtins.unsafeDiscardStringContext "exec ${exe} ") (init c);
  fishExe = c: lib.getExe c.programs.fish.package;
  zshExe = c: "${c.programs.zsh.package}/bin/zsh";
  nuExe = c: lib.getExe c.programs.nushell.package;
  others =
    c:
    lib.count lib.id [
      c.programs.fish.enable
      c.programs.zsh.enable
      c.programs.nushell.enable
    ];
  fish =
    c:
    c.programs.fish.enable
    && others c == 1
    && handsOver c (fishExe c) "fish"
    && lib.elem c.programs.fish.package c.environment.systemPackages
    && hotaruShell c == "${fishExe c}\n";
  bash =
    c:
    others c == 0
    && !(lib.hasInfix "HOTARU_SHELL_PARENT" (init c))
    && hotaruShell c == "${lib.getExe pkgs.bashInteractive}\n";
  zshell =
    c:
    c.programs.zsh.enable
    && others c == 1
    && handsOver c (zshExe c) "zsh"
    && c.programs.zsh.package == applicationCatalog.specialArgs.applicationPkgs.zsh
    && hotaruShell c == "${zshExe c}\n"
    # Omarchy's eza, not NixOS's ls alias set after it.
    && !(c.programs.zsh.shellAliases ? ls && c.programs.zsh.shellAliases.ls != null)
    && lib.hasInfix "starship init zsh" c.programs.zsh.promptInit
    && c.programs.zsh.autosuggestions.enable
    && c.programs.zsh.syntaxHighlighting.enable;
  nu =
    c:
    c.programs.nushell.enable
    && others c == 1
    && handsOver c (nuExe c) "nu"
    && c.programs.nushell.package == applicationCatalog.specialArgs.applicationPkgs.nushell
    && hotaruShell c == "${nuExe c}\n"
    && lib.elem "/share/nushell" c.environment.pathsToLink
    && lib.hasPrefix "source /nix/store/" c.programs.nushell.interactiveShellInit;
  json = file: builtins.fromJSON (builtins.unsafeDiscardStringContext file.text);
  manifest = c: json c.environment.etc."installer-applications.json";
  choices = c: (json c.environment.etc."yukimi/system.json").choices;
  choice = c: lib.findFirst (choice: choice.setting == "calamares.terminalShell") null (choices c);
in
assert fish aizome && fish together && fish chosen && fish reconsidered;
assert bash tatami && bash plasma && bash declined && bash declinedWithFish;
assert zshell zsh && zshell replaced && nu nushell;
assert lib.elem pkgs.fish declinedWithFish.environment.systemPackages;
# fish is no longer among the applications, for Genkan or Yukimi's Discover.
assert !(lib.any (app: app.id == "fish") applicationCatalog.catalog);
assert (manifest aizome).selected == [ ] && (manifest chosen).selected == [ "firefox" ];
# Yukimi offers the four shells, one choice in place of the fish switch.
assert lib.all (c: !(lib.any (choice: choice.setting == "calamares.fish.enable") (choices c))) [
  aizome
  tatami
];
assert (choice aizome).value == "fish" && (choice declined).value == "bash";
assert (choice tatami).value == "bash" && (choice chosen).value == "fish";
assert (choice zsh).value == "zsh" && (choice nushell).value == "nushell";
assert (choice reconsidered).value == "fish" && (choice replaced).value == "zsh";
assert
  !((choice aizome).boolean or false)
  && !((choice aizome).many or false)
  &&
    map (option: option.id) (choice aizome).options == [
      "fish"
      "bash"
      "zsh"
      "nushell"
    ];
# Force module assertions, not just the options above.
assert builtins.all (c: c.system.build.toplevel.drvPath != "") [
  aizome
  tatami
  chosen
  zsh
  nushell
];
pkgs.writeText "terminal-shell-default.json" (
  builtins.toJSON (
    lib.mapAttrs (_: c: c.calamares.terminalShell) {
      inherit
        aizome
        together
        tatami
        plasma
        chosen
        declined
        declinedWithFish
        zsh
        nushell
        reconsidered
        replaced
        ;
    }
  )
)
