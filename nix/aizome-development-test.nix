{ pkgs }:
pkgs.runCommand "aizome-development"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.bash
      pkgs.git
    ];
    AIZOME_DEV_HOOK = ../desktop/aizome/scripts/dev-environment;
  }
  ''
    python3 ${../tests/test_aizome_development.py}
    touch $out
  ''
