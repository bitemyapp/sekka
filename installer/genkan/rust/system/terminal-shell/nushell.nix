# SPDX-License-Identifier: GPL-3.0-or-later
# nushell, when terminals open in it (default.nix): Omarchy's Starship
# prompt, zoxide's z and fzf's key bindings, without the welcome banner.
# Nushell runs this vendor autoload after your config.nu; files in your own
# autoload directory (~/.config/nushell/autoload) run after it and can
# change any of it.
{
  config,
  lib,
  pkgs,
  applicationPkgs ? pkgs,
  ...
}:
let
  inherit (applicationPkgs)
    nushell
    starship
    zoxide
    fzf
    ;
  omarchyPrompt = import ./prompt.nix { inherit lib; };
  # Nushell can only source files it can read before running anything, so
  # the prompt's and zoxide's scripts are made here rather than at each
  # start. Unlike mise's, they don't capture the environment they're made in.
  init =
    pkgs.runCommand "sekka-nushell-init"
      {
        nativeBuildInputs = [
          nushell
          starship
          zoxide
        ];
      }
      ''
        export HOME=$TMPDIR
        {
          cat <<'EOF'
        $env.config.show_banner = false
        # Omarchy's prompt, unless you have your own.
        if ($env.STARSHIP_CONFIG? | is-empty) and not ("~/.config/starship.toml" | path expand | path exists) {
          $env.STARSHIP_CONFIG = "${omarchyPrompt}/starship.toml"
        }
        EOF
          starship init nu
          zoxide init nushell
          cat ${fzf}/share/fzf/key-bindings.nu
        } > $out
        # Parsed and run with this nushell, so a script it no longer accepts
        # fails the build rather than every terminal.
        nu --no-config-file --commands "source $out"
      '';
in
lib.mkIf (config.calamares.terminalShell == "nushell") {
  # zoxide and fzf are called by name.
  environment.systemPackages = [
    zoxide
    fzf
  ];
  programs.nushell = {
    enable = true;
    package = nushell;
    interactiveShellInit = "source ${init}";
  };
}
