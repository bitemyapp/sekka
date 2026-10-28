# Focused native-terminal build; shares Aizome's toolkit, theme and lockfile.
# Only the app and its required libraries are compiled, not the compositor.
{
  lib,
  stdenv,
  rustPlatform,
  runCommand,
  patch,
  pkg-config,
  makeWrapper,
  wayland,
  libxkbcommon,
  fontconfig,
  freetype,
  libglvnd,
  makeFontsConf,
  noto-fonts-cjk-sans,
  src,
  yukimiSrc,
  hotaruSrc,
  clipboardPatch,
  inputPatch,
  mimePatch,
  sidestep,
  fonts,
  version,
}:
let
  source = runCommand "hotaru-source" { nativeBuildInputs = [ patch ]; } ''
    mkdir -p $out/desktop $out/apps $out/vendor
    cp -r ${src} $out/desktop/aizome
    cp -r ${yukimiSrc} $out/apps/yukimi
    cp -r ${hotaruSrc} $out/apps/hotaru
    cp -r ${sidestep} $out/vendor/sidestep
    chmod -R u+w $out/vendor/sidestep
    patch -d $out/vendor/sidestep -p1 < ${clipboardPatch}
    patch -d $out/vendor/sidestep -p1 < ${inputPatch}
    patch -d $out/vendor/sidestep -p1 < ${mimePatch}
    chmod u+w $out/desktop/aizome
    ln -sfn ../../vendor/sidestep $out/desktop/aizome/sidestep
  '';
in
rustPlatform.buildRustPackage {
  pname = "hotaru";
  inherit version;
  src = source;
  sourceRoot = "hotaru-source/desktop/aizome";
  cargoLock.lockFile = "${src}/Cargo.lock";
  nativeBuildInputs = [
    pkg-config
    makeWrapper
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
    "aizome-hotaru"
  ];
  cargoTestFlags = [
    "-p"
    "aizome-hotaru"
  ];
  buildType = "release";
  # Only this app is selected; unlike the compositor's dylibs it can use LTO.
  CARGO_PROFILE_RELEASE_LTO = "thin";
  CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "1";
  postPatch = ''
    substituteInPlace apps/hotaru/tests/native.rs --replace-fail /bin/sh ${stdenv.shell}
  '';
  preCheck = ''
    export XDG_CACHE_HOME=$TMPDIR/hotaru-test-cache
    mkdir -p "$XDG_CACHE_HOME"
    export AIZOME_FONT_DIR=${fonts}
    export FONTCONFIG_FILE=${
      makeFontsConf {
        fontDirectories = [
          fonts
          noto-fonts-cjk-sans
        ];
      }
    }
    export LD_LIBRARY_PATH=${
      lib.makeLibraryPath [
        libglvnd
        fontconfig
        libxkbcommon
        wayland
      ]
    }
  '';
  installPhase = ''
    runHook preInstall
    install -Dm755 target/${stdenv.hostPlatform.rust.rustcTarget}/release/aizome-hotaru $out/bin/hotaru
    ln -s hotaru $out/bin/aizome-hotaru
    install -Dm644 apps/hotaru/data/org.aizome.Hotaru.desktop $out/share/applications/org.aizome.Hotaru.desktop
    install -Dm644 apps/hotaru/data/org.aizome.Hotaru.svg $out/share/icons/hicolor/scalable/apps/org.aizome.Hotaru.svg
    runHook postInstall
  '';
  postFixup = ''
    wrapProgram $out/bin/hotaru \
      --set-default AIZOME_FONT_DIR ${fonts} \
      --prefix LD_LIBRARY_PATH : ${
        lib.makeLibraryPath [
          libglvnd
          fontconfig
          libxkbcommon
          wayland
        ]
      }
  '';
  meta = {
    description = "Hotaru, Aizome's native Rust terminal (in development)";
    license = with lib.licenses; [
      mit
      asl20
    ];
    platforms = lib.platforms.linux;
    mainProgram = "hotaru";
  };
}
