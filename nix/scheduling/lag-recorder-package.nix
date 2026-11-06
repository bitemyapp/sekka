# SPDX-License-Identifier: GPL-3.0-or-later
{ pkgs }:
let
  source = pkgs.runCommand "sekka-lag-recorder-source" { } ''
    mkdir -p $out
    cp ${./lag-recorder.py} $out/lag-recorder.py
    cp ${./lag_history.py} $out/lag_history.py
    cp ${./lag_summary.py} $out/lag_summary.py
  '';
in
pkgs.writeShellScriptBin "sekka-lag-recorder" ''
  export PATH=${pkgs.lib.makeBinPath [ pkgs.systemd ]}
  exec ${pkgs.python3}/bin/python3 ${source}/lag-recorder.py "$@"
''
