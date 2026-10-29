{ pkgs }:
pkgs.runCommand "aizome-session-launcher"
  {
    nativeBuildInputs = [
      pkgs.python3
      pkgs.shellcheck
    ];
    AIZOME_SESSION_LAUNCHER = ../session/aizome-session;
  }
  ''
    shellcheck "$AIZOME_SESSION_LAUNCHER"
    python3 ${./session-launcher-test.py} -v
    touch $out
  ''
