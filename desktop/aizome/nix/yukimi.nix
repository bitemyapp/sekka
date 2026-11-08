# Yukimi's app (apps/yukimi), Sekka's software manager on every desktop,
# built alone from the crates it uses (workspace.nix): an edit to the
# compositor or another app changes neither this package nor the systems
# that include it. Aizome's package links it for its session.
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
  libpulseaudio,
  libsecret,
  dbus,
  bash,
  weston,
  makeFontsConf,
  noto-fonts-cjk-sans,
  tzdata,
  fonts,
  hotaruFonts,
  workspace,
  version,
}:
let
  target = stdenv.hostPlatform.rust.rustcTarget;
  # As Aizome's: the libraries the toolkit opens with dlopen.
  runtimeLibs = [
    libglvnd
    libpulseaudio
    libsecret
    fontconfig
    libxkbcommon
    wayland
  ];
  test = "cargo test --offline --target ${target} --profile release-lto -p aizome-yukimi";
  # Headless interactions and rendering, without a desktop.
  testEnvironment = ''
    export AIZOME_FONT_DIR=${fonts}
    export HOTARU_FONT_DIR=${hotaruFonts}
    export LD_LIBRARY_PATH=${lib.makeLibraryPath runtimeLibs}
    export FONTCONFIG_FILE=${
      makeFontsConf {
        fontDirectories = [
          fonts
          noto-fonts-cjk-sans
        ];
      }
    }
    export TZDIR=${tzdata}/share/zoneinfo
  '';
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
    buildPhase = ''
      runHook preBuild
      cargo build -j "$NIX_BUILD_CORES" --target ${target} --offline --profile release-lto -p aizome-yukimi
      runHook postBuild
    '';
    doCheck = true;
    nativeCheckInputs = [ dbus ];
  };
  dependencies = rustPlatform.buildRustPackage (
    common
    // {
      pname = "aizome-yukimi-dependencies";
      src = workspace.stubs;
      checkPhase = ''
        runHook preCheck
        ${testEnvironment}
        ${test} --no-run --lib
        runHook postCheck
      '';
      installPhase = workspace.saveTarget;
      dontFixup = true;
    }
  );
in
rustPlatform.buildRustPackage (
  common
  // {
    pname = "aizome-yukimi";
    src = workspace.withCrates {
      crates = [ "desktop/aizome/apps/yukimi" ];
      extras = [ ../icons/apps/org.aizome.Yukimi.svg ];
    };
    preBuild = workspace.sameEnvironment + workspace.restoreTarget dependencies;
    checkPhase = ''
      runHook preCheck
      ${testEnvironment}
      ${test} --lib --test headless --test look --test golden
      runHook postCheck
    '';
    installPhase = ''
      runHook preInstall
      install -Dm755 target/${target}/release-lto/aizome-yukimi $out/libexec/aizome-yukimi
      mkdir -p $out/bin
      substitute apps/yukimi/data/yukimi-launch $out/bin/aizome-yukimi \
        --subst-var-by bash ${bash}/bin/bash \
        --subst-var-by app "$out/libexec/aizome-yukimi" \
        --subst-var-by weston ${weston}/bin/weston
      chmod +x $out/bin/aizome-yukimi
      ln -s aizome-yukimi $out/bin/yukimi
      install -Dm644 apps/yukimi/data/org.aizome.Yukimi.desktop $out/share/applications/org.aizome.Yukimi.desktop
      install -Dm644 icons/apps/org.aizome.Yukimi.svg $out/share/icons/hicolor/scalable/apps/org.aizome.Yukimi.svg
      runHook postInstall
    '';
    postFixup = ''
      patchelf --add-rpath ${lib.makeLibraryPath runtimeLibs} $out/libexec/aizome-yukimi
      wrapProgram $out/libexec/aizome-yukimi --set-default AIZOME_FONT_DIR ${fonts}
    '';
    passthru = { inherit dependencies; };
    meta = {
      description = "Yukimi, Sekka's software manager, as Aizome's native app";
      license = with lib.licenses; [
        mit
        asl20
      ];
      mainProgram = "aizome-yukimi";
      platforms = lib.platforms.linux;
    };
  }
)
