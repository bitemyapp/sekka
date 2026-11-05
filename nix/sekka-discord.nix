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
    ];
  };
  cargoLock.lockFile = ../services/sekka-discord/Cargo.lock;
  nativeBuildInputs = [ pkgs.pkg-config ];
  buildInputs = [ pkgs.sqlite ];
  doCheck = true;
  postInstall = ''
    install -Dm644 ${../services/sekka-discord/sekka-discord.service} \
      "$out/share/systemd/user/sekka-discord.service"
    install -Dm644 ${../services/sekka-discord/sekka-discord-tunnel.service} \
      "$out/share/systemd/user/sekka-discord-tunnel.service"
  '';
  meta = {
    description = "Discord notifications for the Sekka GitHub repository";
    license = pkgs.lib.licenses.asl20;
    platforms = [ "x86_64-linux" ];
    mainProgram = "sekka-discord";
  };
}
