# SPDX-License-Identifier: GPL-3.0-or-later
{ pkgs }:
pkgs.writeShellScriptBin "sekka-lag-recorder" ''
  export PATH=${pkgs.lib.makeBinPath [ pkgs.systemd ]}
  exec ${pkgs.python3}/bin/python3 ${./lag-recorder.py} "$@"
''
