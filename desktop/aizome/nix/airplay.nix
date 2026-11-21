# Native AirPlay window, compiled independently from the desktop. Reuse the
# workspace's manifest/lock cache and only include this app's path closure.
{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  makeWrapper,
  zstd,
  wayland,
  libxkbcommon,
  fontconfig,
  freetype,
  libglvnd,
  workspace,
  fonts,
  doubletake,
  airplay-peer-session,
  version,
}:
let
  target = stdenv.hostPlatform.rust.rustcTarget;
  runtimeLibs = [
    libglvnd
    fontconfig
    libxkbcommon
    wayland
  ];
  common = {
    inherit version;
    sourceRoot = "aizome-source/desktop/aizome";
    cargoDeps = workspace.vendor;
    preBuild = workspace.sameEnvironment;
    nativeBuildInputs = [
      pkg-config
      makeWrapper
      zstd
      rustPlatform.bindgenHook
    ];
    buildInputs = [
      wayland
      libxkbcommon
      fontconfig
      freetype
    ];
    cargoBuildFlags = [
      "-p"
      "aizome-kagami"
    ];
    cargoTestFlags = [
      "-p"
      "aizome-kagami"
    ];
    buildType = "release";
    CARGO_PROFILE_RELEASE_LTO = "thin";
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "1";
    preCheck = ''
      export AIZOME_FONT_DIR=${fonts}
      export LD_LIBRARY_PATH=${lib.makeLibraryPath runtimeLibs}
    '';
  };
  dependencies = rustPlatform.buildRustPackage (
    common
    // {
      pname = "sekka-airplay-dependencies";
      src = workspace.stubs;
      cargoTestFlags = common.cargoTestFlags ++ [ "--no-run" ];
      installPhase = workspace.saveTarget;
      dontFixup = true;
    }
  );
in
rustPlatform.buildRustPackage (
  common
  // {
    pname = "sekka-airplay";
    src = workspace.withCrates { crates = [ "desktop/aizome/apps/kagami" ]; };
    preBuild = workspace.sameEnvironment + workspace.restoreTarget dependencies;
    postBuild = workspace.onlyFirstParty;
    postCheck = workspace.onlyFirstParty;
    installPhase = ''
      runHook preInstall
      install -Dm755 target/${target}/release/aizome-kagami $out/bin/sekka-airplay
      install -Dm644 apps/kagami/data/computer.sekka.AirPlay.desktop -t $out/share/applications
      runHook postInstall
    '';
    postFixup = ''
      wrapProgram $out/bin/sekka-airplay \
        --set-default AIZOME_FONT_DIR ${fonts} \
        --set-default SEKKA_AIRPLAY_SENDER ${doubletake}/bin/doubletake \
        --set-default SEKKA_AIRPLAY_PEER_HELPER ${airplay-peer-session}/bin/sekka-airplay-peer-session \
        --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
    '';
    passthru = { inherit dependencies doubletake airplay-peer-session; };
    meta = {
      description = "Mirror a screen and laptop audio to an AirPlay TV";
      license = [
        lib.licenses.mit
        lib.licenses.asl20
      ];
      platforms = lib.platforms.linux;
      mainProgram = "sekka-airplay";
    };
  }
)
