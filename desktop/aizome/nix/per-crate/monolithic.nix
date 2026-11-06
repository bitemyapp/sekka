# The baseline for the per-crate experiment: the same two packages (the
# compositor and its logic) in one derivation and one Cargo invocation, the
# way ../package.nix's first build compiles them, without the rest of the
# workspace, its tests or its installation.
{
  lib,
  stdenv,
  rustPlatform,
  runCommand,
  pkg-config,
  wayland,
  libxkbcommon,
  libinput,
  udev,
  seatd,
  libdrm,
  libgbm,
  libglvnd,
  mesa,
  pixman,
  fontconfig,
  freetype,
  dbus,
  pam,
  libpulseaudio,
  src,
  yukimiSrc,
  hotaruSrc,
}:
let
  source = runCommand "aizome-source" { } ''
    mkdir -p $out/desktop $out/apps
    cp -r ${src} $out/desktop/aizome
    cp -r ${yukimiSrc} $out/apps/yukimi
    cp -r ${hotaruSrc} $out/apps/hotaru
  '';
  target = stdenv.hostPlatform.rust.rustcTarget;
in
rustPlatform.buildRustPackage {
  pname = "aizome-compositor-monolithic";
  version = "0.1.0";
  src = source;
  sourceRoot = "aizome-source/desktop/aizome";
  cargoLock = {
    lockFile = "${src}/Cargo.lock";
    outputHashes = import ../cargo-output-hashes.nix;
  };
  nativeBuildInputs = [
    pkg-config
    rustPlatform.bindgenHook
  ];
  buildInputs = [
    wayland
    libxkbcommon
    libinput
    udev
    seatd
    libdrm
    libgbm
    libglvnd
    mesa
    pixman
    fontconfig
    freetype
    dbus
    pam
    libpulseaudio
  ];
  postPatch = ''
    patchShebangs scripts
  '';
  buildPhase = ''
    runHook preBuild
    export RUSTC_WRAPPER=$PWD/scripts/rustc-wrapper
    cargo build -j "$NIX_BUILD_CORES" --target ${target} --offline --release -p aizome-comp -p aizome-comp-logic
    runHook postBuild
  '';
  doCheck = false;
  installPhase = ''
    runHook preInstall
    rel=target/${target}/release
    install -Dm755 $rel/aizome $out/bin/aizome
    install -Dm644 -t $out/lib/aizome $rel/libaizome_comp_core.so $rel/libaizome_comp_logic.so
    runHook postInstall
  '';
}
