# SPDX-License-Identifier: GPL-3.0-or-later
# fish, set up as Omarchy sets it up (its opt-in omarchy-fish,
# omarchy-fish.nix here) with the tools that configuration calls, when
# terminals open in it (default.nix).
{
  config,
  lib,
  pkgs,
  applicationPkgs ? pkgs,
  ...
}:
let
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
  omarchyFish = pkgs.callPackage ./omarchy-fish.nix { };
  omarchyPrompt = import ./prompt.nix { inherit lib; };
in
lib.mkIf (config.calamares.terminalShell == "fish") {
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
}
