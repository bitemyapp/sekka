{ pkgs }:
pkgs.runCommand "installer-live-diagnostics"
  {
    nativeBuildInputs = [
      pkgs.rustc
      pkgs.stdenv.cc
      pkgs.makeWrapper
    ];
  }
  ''
    mkdir -p $out/bin
    rustc --edition=2024 -O ${../scripts/live_diagnostics.rs} -o $out/bin/installer-live-diagnostics
    wrapProgram $out/bin/installer-live-diagnostics --prefix PATH : ${
      pkgs.lib.makeBinPath [
        pkgs.coreutils
        pkgs.systemd
        pkgs.pciutils
        pkgs.usbutils
        pkgs.libinput
        pkgs.util-linux
      ]
    }
  ''
