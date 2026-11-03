# Focused native-terminal build; shares Aizome's toolkit (Sukumo, inside
# desktop/aizome), theme and lockfile.
# Only the app and its required libraries are compiled, not the compositor,
# from a source with only their crates real (workspace.nix): an edit to the
# compositor or another app leaves this package as it was. Third-party
# crates compile first, in `dependencies`.
{
  lib,
  stdenv,
  rustPlatform,
  pkg-config,
  makeWrapper,
  ncurses,
  zstd,
  wayland,
  libxkbcommon,
  fontconfig,
  freetype,
  libglvnd,
  makeFontsConf,
  noto-fonts-cjk-sans,
  ghosttyLicense,
  uucodeLicense,
  unicodeLicense,
  fonts,
  hotaruFonts,
  workspace,
  version,
}:
let
  # What the dependency build and the package's share, so Cargo finds the
  # dependencies fresh.
  common = {
    inherit version;
    sourceRoot = "aizome-source/desktop/aizome";
    cargoDeps = workspace.vendor;
    preBuild = workspace.sameEnvironment;
    nativeBuildInputs = [
      pkg-config
      makeWrapper
      ncurses
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
    preCheck = ''
      export XDG_CACHE_HOME=$TMPDIR/hotaru-test-cache
      mkdir -p "$XDG_CACHE_HOME"
      export AIZOME_FONT_DIR=${fonts}
      export HOTARU_FONT_DIR=${hotaruFonts}
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
  };
  dependencies = rustPlatform.buildRustPackage (
    common
    // {
      pname = "hotaru-dependencies";
      src = workspace.stubs;
      # Compile the tests' dependencies without running anything.
      cargoTestFlags = common.cargoTestFlags ++ [ "--no-run" ];
      installPhase = workspace.saveTarget;
      dontFixup = true;
    }
  );
in
rustPlatform.buildRustPackage (
  common
  // {
    pname = "hotaru";
    src = workspace.withCrates {
      crates = [ "desktop/aizome/apps/hotaru" ];
      extras = [ ../../../apps/hotaru/resources ];
    };
    preBuild = workspace.sameEnvironment + workspace.restoreTarget dependencies;
    postPatch = ''
      substituteInPlace apps/hotaru/tests/native.rs --replace-fail /bin/sh ${stdenv.shell}
    '';
    installPhase = ''
      runHook preInstall
      install -Dm755 target/${stdenv.hostPlatform.rust.rustcTarget}/release/aizome-hotaru $out/bin/hotaru
      ln -s hotaru $out/bin/aizome-hotaru
      install -Dm644 apps/hotaru/data/org.aizome.Hotaru.desktop $out/share/applications/org.aizome.Hotaru.desktop
      install -Dm644 apps/hotaru/data/org.aizome.Hotaru.svg $out/share/icons/hicolor/scalable/apps/org.aizome.Hotaru.svg
      install -Dm644 ${ghosttyLicense} $out/share/licenses/hotaru/Ghostty-MIT.txt
      install -m644 crates/hotaru-link-regex/vendor/oniguruma/LICENSE $out/share/licenses/hotaru/Oniguruma-BSD-2-Clause.txt
      install -Dm644 ${uucodeLicense} $out/share/licenses/hotaru/uucode-MIT.txt
      install -Dm644 ${unicodeLicense} $out/share/licenses/hotaru/Unicode-3.0.txt
      install -m644 ../../apps/hotaru/crates/hotaru-sprites/LICENSE.MPL-2.0 $out/share/licenses/hotaru/z2d-MPL-2.0.txt
      install -m644 ../../apps/hotaru/crates/hotaru-sprites/LICENSE.z2d $out/share/licenses/hotaru/z2d-NOTICES.txt
      install -m644 ../../apps/hotaru/crates/hotaru-sprites/LICENSE.zig $out/share/licenses/hotaru/Zig-MIT.txt
      install -m644 ../../apps/hotaru/vendor/simdutf8/LICENSE-MIT $out/share/licenses/hotaru/simdutf8-MIT.txt
      install -m644 ../../apps/hotaru/vendor/simdutf8/LICENSE-Apache $out/share/licenses/hotaru/simdutf8-Apache.txt
      mkdir -p $out/share/hotaru $out/share/terminfo
      cp -r ../../apps/hotaru/resources/shell-integration $out/share/hotaru/
      install -m644 ../../apps/hotaru/resources/LICENSE-MIT $out/share/hotaru/LICENSE-MIT
      install -m644 ../../apps/hotaru/resources/LICENSE-MIT $out/share/licenses/hotaru/Shell-integration-MIT.txt
      install -m644 ../../apps/hotaru/resources/hotaru.terminfo $out/share/hotaru/hotaru.terminfo
      tic -x -o $out/share/terminfo $out/share/hotaru/hotaru.terminfo
      runHook postInstall
    '';
    postFixup = ''
      wrapProgram $out/bin/hotaru \
        --set-default AIZOME_FONT_DIR ${fonts} \
        --set-default HOTARU_FONT_DIR ${hotaruFonts} \
        --set-default HOTARU_RESOURCES_DIR $out/share/hotaru \
        --set-default HOTARU_TERMINFO_DIR $out/share/terminfo \
        --prefix LD_LIBRARY_PATH : ${
          lib.makeLibraryPath [
            libglvnd
            fontconfig
            libxkbcommon
            wayland
          ]
        }
    '';
    passthru = { inherit dependencies; };
    meta = {
      description = "Hotaru, Aizome's native Rust terminal (in development)";
      license = with lib.licenses; [
        mpl20
        bsd2
        mit
        asl20
      ];
      platforms = lib.platforms.linux;
      mainProgram = "hotaru";
    };
  }
)
