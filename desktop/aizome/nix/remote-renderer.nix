# Local first-party Sukumo paint replay, with a C ABI for native Remote Desks
# viewers. No compositor, session helpers, or application workspace is linked.
{ lib, rustPlatform, pkg-config, wayland, fontconfig }:
rustPlatform.buildRustPackage {
  pname = "aizome-remote-renderer";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ../sukumo;
    fileset = lib.fileset.unions [
      ../sukumo/Cargo.toml
      ../sukumo/Cargo.lock
      ../sukumo/crates/aizome-remote-renderer
      ../sukumo/crates/aizome-remote-protocol
      ../sukumo/crates/sidestep-engine
    ];
  };
  cargoLock.lockFile = ../sukumo/Cargo.lock;
  cargoBuildFlags = [ "-p" "aizome-remote-renderer" ];
  cargoTestFlags = [ "-p" "aizome-remote-renderer" "-p" "aizome-remote-protocol" ];
  nativeBuildInputs = [ pkg-config ];
  buildInputs = [ wayland fontconfig ];
  installPhase = ''
    runHook preInstall
    install -Dm755 target/*/release/libaizome_remote_renderer.so $out/lib/libaizome_remote_renderer.so
    install -Dm644 crates/aizome-remote-protocol/protocol/remote-paint.xml $out/share/wayland-protocols/aizome/remote-paint.xml
    runHook postInstall
  '';
  meta = {
    description = "Bounded native replay of Aizome/Sukumo retained drawing";
    license = with lib.licenses; [ mit asl20 ];
    platforms = lib.platforms.linux;
  };
}
