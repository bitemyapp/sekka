{ nixpkgs, system }:
let
  pkgs = nixpkgs.legacyPackages.${system};
  configuration =
    extra:
    (nixpkgs.lib.nixosSystem {
      inherit system;
      modules = [
        ../installer/genkan/rust/system/gaming.nix
        { users.users.player.isNormalUser = true; }
        extra
      ];
    }).config;
  enabled = configuration { programs.steam.enable = true; };
  absent = configuration { };
  tearing = configuration {
    programs.steam.enable = true;
    calamares.gaming.gamescopeSession.allowTearing = true;
  };
  disabled = configuration {
    programs.steam.enable = true;
    calamares.gaming.enable = false;
  };
  custom = configuration {
    programs.steam.enable = true;
    calamares.gaming = {
      gamescopeSession.enable = false;
      protonGE.enable = false;
    };
    programs.gamemode.enable = false;
    programs.gamescope.package = pkgs.gamescope;
  };
in
assert enabled.hardware.graphics.enable32Bit;
assert enabled.hardware.steam-hardware.enable;
assert enabled.programs.gamemode.enable;
assert !enabled.programs.gamemode.enableRenice;
assert enabled.users.groups.gamemode.members == [ "player" ];
assert enabled.programs.gamescope.enable;
assert enabled.programs.gamescope.enableWsi;
assert builtins.length enabled.hardware.graphics.extraPackages32 > 0;
assert !enabled.programs.gamescope.capSysNice;
assert enabled.programs.steam.gamescopeSession.enable;
assert
  enabled.programs.steam.gamescopeSession.args == [
    "--adaptive-sync"
    "--hdr-enabled"
  ];
assert builtins.length enabled.programs.steam.extraCompatPackages == 1;
assert builtins.elem "--immediate-flips" tearing.programs.steam.gamescopeSession.args;
assert !enabled.programs.steam.remotePlay.openFirewall;
assert !enabled.programs.steam.localNetworkGameTransfers.openFirewall;
assert !absent.programs.gamemode.enable && !absent.programs.gamescope.enable;
assert !disabled.programs.gamemode.enable && !disabled.programs.gamescope.enable;
assert !custom.programs.gamemode.enable;
assert custom.programs.gamescope.package.outPath == pkgs.gamescope.outPath;
assert !custom.programs.steam.gamescopeSession.enable;
assert custom.programs.steam.extraCompatPackages == [ ];
pkgs.runCommand "sekka-gaming-policy"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.bash
      pkgs.shellcheck
    ];
  }
  ''
    shellcheck -s bash ${../installer/genkan/rust/system/gaming/sekka-game.sh}
    shellcheck ${../tests/gaming-gpu-smoke.sh}
    SEKKA_GAME_SCRIPT=${../installer/genkan/rust/system/gaming/sekka-game.sh} \
      python3 ${../tests/test_gaming.py}
    touch $out
  ''
