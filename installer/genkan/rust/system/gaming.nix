# SPDX-License-Identifier: GPL-3.0-or-later
# A desktop Steam installation and an optional Gamescope login session.
# All graphics packages follow the system lock, including their 32-bit halves.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.calamares.gaming;
  gamescopeCfg = config.programs.gamescope;
  # programs.gamescope's own wrapper, for its args and env, without its
  # optional capability wrapper: that cannot run inside Steam's sandbox.
  gamescope = pkgs.runCommand "sekka-gamescope" { nativeBuildInputs = [ pkgs.makeBinaryWrapper ]; } ''
    mkdir -p $out/bin
    makeWrapper ${gamescopeCfg.package}/bin/gamescope $out/bin/gamescope --inherit-argv0 \
      ${lib.escapeShellArgs (
        lib.optionals (gamescopeCfg.args != [ ]) [
          "--add-flags"
          (toString gamescopeCfg.args)
        ]
        ++ lib.concatLists (
          lib.mapAttrsToList (name: value: [
            "--set-default"
            name
            value
          ]) gamescopeCfg.env
        )
      )}
  '';
  launcher = pkgs.writeShellApplication {
    name = "sekka-game";
    runtimeInputs = [
      pkgs.gamemode
      pkgs.mangohud
      gamescope
    ];
    text = builtins.readFile ./gaming/sekka-game.sh;
  };
in
{
  options.calamares.gaming = {
    enable = lib.mkOption {
      type = lib.types.bool;
      default = config.programs.steam.enable;
      defaultText = lib.literalExpression "config.programs.steam.enable";
      description = "Steam gaming tools, on-demand GameMode and a Gamescope session.";
    };
    gamescopeSession.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Offer Steam as a separate Gamescope login session with HDR and adaptive sync.";
    };
    gamescopeSession.allowTearing = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Allow immediate flips in the Steam session. May reduce latency at the cost of visible tearing.";
    };
    protonGE.enable = lib.mkOption {
      type = lib.types.bool;
      default = true;
      description = "Make the pinned GE-Proton available in Steam's compatibility menu.";
    };
  };

  config = lib.mkIf cfg.enable {
    # Nested Gamescope and its WSI layer stay available without the login
    # session. Steam's module sets this to the session's state with
    # mkDefault, so a plain mkDefault would conflict once the session is off.
    # An explicit user setting still wins.
    programs.gamescope.enable = lib.mkOverride 900 true;
    programs.gamescope.package = lib.mkDefault (
      import ../../../../nix/gamescope.nix {
        inherit (pkgs) gamescope;
      }
    );
    programs.gamescope.enableWsi = lib.mkDefault true;
    programs.gamemode = {
      enable = lib.mkDefault true;
      # Ananicy already owns process priorities. GameMode requests a CPU
      # performance governor only while a game opts in, and restores it.
      enableRenice = lib.mkDefault false;
      settings.general = {
        renice = lib.mkDefault 0;
        desiredgov = lib.mkDefault "performance";
        inhibit_screensaver = lib.mkDefault 1;
      };
    };
    users.groups.gamemode.members = lib.attrNames (
      lib.filterAttrs (_: user: user.isNormalUser) config.users.users
    );
    programs.steam = lib.mkIf config.programs.steam.enable {
      extraPackages = [
        launcher
        pkgs.gamemode
        pkgs.mangohud
        gamescope
      ];
      extraCompatPackages = lib.optionals cfg.protonGE.enable [ pkgs.proton-ge-bin ];
      gamescopeSession = lib.mkIf cfg.gamescopeSession.enable {
        enable = lib.mkDefault true;
        # Gamescope enables HDR only when the physical output supports it.
        # Unlike a nested window, this session owns KMS and can set HDR
        # metadata, adaptive sync and asynchronous flips itself.
        args = [
          "--adaptive-sync"
          "--hdr-enabled"
        ]
        ++ lib.optional cfg.gamescopeSession.allowTearing "--immediate-flips";
      };
    };
    environment.systemPackages = [
      launcher
      pkgs.mangohud
      pkgs.vulkan-tools
    ];
  };
}
