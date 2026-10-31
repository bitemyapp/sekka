# SPDX-License-Identifier: GPL-3.0-or-later
# Terminals open in fish, set up as Omarchy sets it up (its opt-in
# omarchy-fish, package.nix here) with the tools that configuration calls.
# It's how terminals open wherever Aizome is installed. Elsewhere bash stays,
# as in Omarchy, whose Tatami follows it, unless fish is chosen in Yukimi or
# was chosen among the installer's applications before it came with Aizome.
# Imported by applications.nix, whose packages these are.
{
  config,
  options,
  lib,
  pkgs,
  applicationPkgs ? pkgs,
  ...
}:
let
  cfg = config.calamares.fish;
  # The omarchy-fish PKGBUILD's dependencies, which its functions call.
  tools = with applicationPkgs; [
    eza
    mise
    zoxide
    starship
    fzf
    fd
    bat
  ];
  omarchyFish = pkgs.callPackage ./package.nix { };
  # Omarchy's Starship prompt, with its license.
  omarchyPrompt = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./starship.toml
      ./LICENSE
    ];
  };
in
{
  options.calamares.fish.enable = lib.mkOption {
    type = lib.types.bool;
    default =
      builtins.elem "aizome" (config.calamares.desktops or [ ])
      || builtins.elem "fish" (config.calamares.applications or [ ]);
    defaultText = lib.literalExpression ''
      builtins.elem "aizome" config.calamares.desktops
        || builtins.elem "fish" config.calamares.applications
    '';
    description = ''
      Open terminals in fish, set up as Omarchy sets it up: its Starship
      prompt, vi keys, fzf searches, zoxide and eza. Bash stays the login
      shell and runs scripts. On by default with Aizome, and where "fish"
      was chosen among the installer's applications.
    '';
  };
  config = lib.mkMerge [
    # Yukimi can turn it on or off, after Sekka's desktops and login screen.
    (lib.optionalAttrs (options.programs ? yukimi) {
      programs.yukimi.choices = lib.mkAfter [
        {
          setting = "calamares.fish.enable";
          title = "Terminal shell";
          boolean = true;
          value = lib.boolToString cfg.enable;
          options = [
            {
              id = "true";
              title = "fish";
              detail = "Set up as Omarchy sets it up: Starship prompt, vi keys, fzf search, zoxide and eza.";
            }
            {
              id = "false";
              title = "bash";
              detail = "Terminals open in bash, the login shell.";
            }
          ];
        }
      ];
    })
    (lib.mkIf cfg.enable {
      environment.systemPackages = [ omarchyFish ] ++ tools;
      programs.fish = {
        enable = true;
        package = applicationPkgs.fish;
        # Omarchy has no ls, ll or l aliases. NixOS's would hide its ls (eza)
        # and fish's own ll.
        shellAliases = lib.genAttrs [ "ls" "ll" "l" ] (_: lib.mkOverride 999 null);
        # Omarchy's prompt, unless you have your own.
        interactiveShellInit = ''
          if not set -q STARSHIP_CONFIG; and not test -e ~/.config/starship.toml
            set -gx STARSHIP_CONFIG ${omarchyPrompt}/starship.toml
          end
        '';
      };
      # Bash stays the login shell, and interactive bash hands over to fish as
      # Omarchy's omarchy-setup-fish has ~/.bashrc do: not for bash -c, shells
      # started from fish or nested shells. For people, not root, whose
      # ~/.bashrc Omarchy leaves alone. Hotaru starts fish itself.
      programs.bash.interactiveShellInit = lib.mkBefore ''
        if [[ $EUID -ne 0 && $(${pkgs.procps}/bin/ps --no-header --pid=$PPID --format=comm) != "fish" && -z ''${BASH_EXECUTION_STRING} && ''${SHLVL} == 1 ]]; then
          shopt -q login_shell && LOGIN_OPTION='--login' || LOGIN_OPTION=""
          exec ${lib.getExe config.programs.fish.package} $LOGIN_OPTION
        fi
      '';
    })
  ];
}
