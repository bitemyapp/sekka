{ pkgs }:
pkgs.rustPlatform.buildRustPackage {
  pname = "calamares-storage-tests";
  version = "0.1.0";
  # The backend at its place in Sekka, beside the typestate crate it uses.
  src = pkgs.lib.fileset.toSource {
    root = ../../..;
    fileset = pkgs.lib.fileset.unions [
      ../rust/Cargo.toml
      ../rust/Cargo.lock
      ../rust/reference.nix
      ../rust/src
      ../../../desktop/aizome/crates/aizome-typestate
    ];
  };
  postUnpack = ''
    sourceRoot="$sourceRoot/installer/genkan/rust"
  '';
  cargoLock.lockFile = ../rust/Cargo.lock;
  CALAMARES_TOOL_PATH = pkgs.lib.makeBinPath [
    pkgs.parted
    pkgs.systemd
    pkgs.kmod
    pkgs.util-linux
    pkgs.cryptsetup
    pkgs.e2fsprogs
    pkgs.btrfs-progs
    pkgs.xfsprogs
    pkgs.dosfstools
    pkgs.coreutils
  ];
  buildPhase = ''
    runHook preBuild
    cargo test --release --offline --locked --no-default-features --lib --no-run --target ${pkgs.stdenv.hostPlatform.rust.rustcTarget}
    runHook postBuild
  '';
  doCheck = false;
  installPhase = ''
    mkdir -p $out/bin
    for test in target/${pkgs.stdenv.hostPlatform.rust.rustcTarget}/release/deps/calamares_nixos-*; do
      if [ -x "$test" ] && [ -f "$test" ]; then
        cp "$test" $out/bin/storage-tests
      fi
    done
    test -x $out/bin/storage-tests
  '';
}
