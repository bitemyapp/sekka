{ pkgs }:
pkgs.rustPlatform.buildRustPackage {
  pname = "sekka-discord";
  version = "0.1.0";
  src = pkgs.lib.fileset.toSource {
    root = ../services/sekka-discord;
    fileset = pkgs.lib.fileset.unions [
      ../services/sekka-discord/Cargo.toml
      ../services/sekka-discord/Cargo.lock
      ../services/sekka-discord/src
      ../services/sekka-discord/migrations
      ../services/sekka-discord/monitor.py
      ../services/sekka-discord/test_monitor.py
    ];
  };
  cargoLock.lockFile = ../services/sekka-discord/Cargo.lock;
  nativeBuildInputs = [ pkgs.pkg-config ];
  buildInputs = [ pkgs.sqlite ];
  nativeCheckInputs = [ pkgs.python3 ];
  doCheck = true;
  postCheck = ''
    python3 -m unittest discover -s . -p test_monitor.py
  '';
  postInstall = ''
    install -Dm755 monitor.py "$out/bin/sekka-discord-monitor"
    substituteInPlace "$out/bin/sekka-discord-monitor" \
      --replace-fail '#!/usr/bin/env python3' '#!${pkgs.python3}/bin/python3'
  '';
  meta = {
    description = "Discord notifications for the Sekka GitHub repository";
    license = pkgs.lib.licenses.asl20;
    platforms = [ "x86_64-linux" ];
    mainProgram = "sekka-discord";
  };
}
