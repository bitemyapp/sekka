# SPDX-License-Identifier: GPL-3.0-or-later
# zsh, when terminals open in it (default.nix): Omarchy's Starship prompt,
# its bash aliases and tools (zshrc), fzf's key bindings, and suggestions as
# you type and syntax highlighting, which fish has of its own.
{
  config,
  lib,
  pkgs,
  applicationPkgs ? pkgs,
  ...
}:
let
  # What Omarchy's bash init and aliases call.
  tools = with applicationPkgs; [
    eza
    mise
    zoxide
    starship
    fzf
    bat
  ];
  omarchyPrompt = import ./prompt.nix { inherit lib; };
in
lib.mkIf (config.calamares.terminalShell == "zsh") {
  environment.systemPackages = tools;
  programs.zsh = {
    enable = true;
    package = applicationPkgs.zsh;
    autosuggestions.enable = true;
    syntaxHighlighting.enable = true;
    histSize = 32768;
    # Sekka's settings stand in for the startup files zsh-newuser-install
    # would write, so it doesn't stop people who have none. The zsh/newuser
    # module runs it right after this file; /etc/zshrc lets it be run again
    # by hand.
    shellInit = ''
      [[ -o interactive ]] && zsh-newuser-install() { :; }
    '';
    # Omarchy has no ll or l aliases, and its ls is eza; NixOS's are set
    # after interactiveShellInit and would replace it.
    shellAliases = lib.genAttrs [ "ls" "ll" "l" ] (_: lib.mkOverride 999 null);
    interactiveShellInit = ''
      unfunction zsh-newuser-install 2>/dev/null && autoload -Uz zsh-newuser-install
      source ${./zshrc}
      # Only the key bindings (Ctrl-R, Ctrl-T, Alt-C), as Omarchy's bash.
      source ${applicationPkgs.fzf}/share/fzf/key-bindings.zsh
    '';
    # Omarchy's prompt, unless you have your own.
    promptInit = ''
      if [[ $TERM != dumb ]]; then
        if [[ -z ''${STARSHIP_CONFIG-} && ! -e ~/.config/starship.toml ]]; then
          export STARSHIP_CONFIG=${omarchyPrompt}/starship.toml
        fi
        eval "$(starship init zsh --print-full-init)"
      fi
    '';
  };
}
