# SPDX-License-Identifier: GPL-3.0-or-later
# The shell terminals open in. Wherever Aizome is installed it's fish, set up
# as Omarchy sets it up (fish.nix). Elsewhere bash stays, as in Omarchy, whose
# Tatami follows it, unless fish was chosen among the installer's applications
# before it came with Aizome. Yukimi offers fish, bash, zsh (zsh.nix) and
# nushell (nushell.nix). Bash stays the login shell and runs scripts.
# Imported by applications.nix, whose packages these are.
{
  config,
  options,
  lib,
  pkgs,
  ...
}:
let
  shell = config.calamares.terminalShell;
  # What each choice runs, and the name ps gives it.
  programs = {
    fish = {
      exe = lib.getExe config.programs.fish.package;
      comm = "fish";
    };
    bash = {
      exe = lib.getExe pkgs.bashInteractive;
      comm = "bash";
    };
    zsh = {
      exe = "${config.programs.zsh.package}/bin/zsh";
      comm = "zsh";
    };
    nushell = {
      exe = lib.getExe config.programs.nushell.package;
      comm = "nu";
    };
  };
  chosen = programs.${shell};
in
{
  imports = [
    ./fish.nix
    ./zsh.nix
    ./nushell.nix
  ];
  options.calamares = {
    terminalShell = lib.mkOption {
      type = lib.types.enum [
        "fish"
        "bash"
        "zsh"
        "nushell"
      ];
      default = if config.calamares.fish.enable then "fish" else "bash";
      defaultText = lib.literalExpression ''if config.calamares.fish.enable then "fish" else "bash"'';
      description = ''
        The shell terminals open in. fish is set up as Omarchy sets it up:
        its Starship prompt, vi keys, fzf searches, zoxide and eza. zsh has
        Omarchy's prompt, aliases and tools, with suggestions as you type and
        syntax highlighting. nushell has Omarchy's prompt, zoxide and fzf's
        searches.
        Bash stays the login shell and runs scripts.
      '';
    };
    fish.enable = lib.mkOption {
      type = lib.types.bool;
      default =
        builtins.elem "aizome" (config.calamares.desktops or [ ])
        || builtins.elem "fish" (config.calamares.applications or [ ]);
      defaultText = lib.literalExpression ''
        builtins.elem "aizome" config.calamares.desktops
          || builtins.elem "fish" config.calamares.applications
      '';
      description = ''
        Whether calamares.terminalShell defaults to fish rather than bash: on
        with Aizome, and where "fish" was chosen among the installer's
        applications. Yukimi wrote this before it offered zsh and nushell;
        calamares.terminalShell, when set, takes precedence.
      '';
    };
  };
  config = lib.mkMerge [
    {
      # A default, not a user's Hotaru override. Keep the Bash choice explicit
      # even when another shell is installed separately. Hotaru rereads this
      # for each new pane, so switching the system also updates
      # already-open terminals. Started directly, the shell loads Hotaru's
      # own integration, which a bash handing itself over skips.
      environment.etc."xdg/aizome/hotaru-shell".text = "${chosen.exe}\n";
    }
    # Yukimi can change it, after Sekka's desktops and login screen.
    (lib.optionalAttrs (options.programs ? yukimi) {
      programs.yukimi.choices = lib.mkAfter [
        {
          setting = "calamares.terminalShell";
          title = "Terminal shell";
          value = shell;
          options = [
            {
              id = "fish";
              title = "fish";
              detail = "Set up as Omarchy sets it up: Starship prompt, vi keys, fzf search, zoxide and eza.";
            }
            {
              id = "bash";
              title = "bash";
              detail = "Terminals default to bash. An explicit shell in Hotaru's configuration takes precedence.";
            }
            {
              id = "zsh";
              title = "zsh";
              detail = "Omarchy's Starship prompt and aliases, suggestions as you type, syntax highlighting, fzf search, zoxide and eza.";
            }
            {
              id = "nushell";
              title = "nushell";
              detail = "Structured data in pipelines, with Omarchy's Starship prompt, fzf search and zoxide.";
            }
          ];
        }
      ];
    })
    (lib.mkIf (shell != "bash") {
      # Interactive bash hands over to the chosen shell as Omarchy's
      # omarchy-setup-fish has ~/.bashrc do: not for bash -c, shells started
      # from the chosen one or nested shells, so foot, Ghostty, kitty and the
      # rest, which start the login shell, open it too. For people, not root,
      # whose ~/.bashrc Omarchy leaves alone. Hotaru starts the chosen shell
      # itself; its PID marker prevents this handoff from replacing an
      # explicit Bash.
      programs.bash.interactiveShellInit = lib.mkBefore ''
        if [[ $EUID -ne 0 && $PPID != ''${HOTARU_SHELL_PARENT:-} && $(${pkgs.procps}/bin/ps --no-header --pid=$PPID --format=comm) != "${chosen.comm}" && -z ''${BASH_EXECUTION_STRING} && ''${SHLVL} == 1 ]]; then
          shopt -q login_shell && LOGIN_OPTION='--login' || LOGIN_OPTION=""
          exec ${chosen.exe} $LOGIN_OPTION
        fi
      '';
    })
  ];
}
