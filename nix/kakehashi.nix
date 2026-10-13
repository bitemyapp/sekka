# Kakehashi (apps/kakehashi), the USB writer, built for Linux: its window
# and the helper that writes a drive. macOS and Windows builds come from
# .github/workflows/kakehashi.yml.
{ pkgs }:
let
  inherit (pkgs) lib;
  source = ../apps/kakehashi;
  # What GPUI links against.
  libraries = with pkgs; [
    fontconfig
    freetype
    libxkbcommon
    wayland
    libxcb
    libx11
    vulkan-loader
  ];
  # What GPUI opens at run time rather than links.
  runtimeLibraries = with pkgs; [
    vulkan-loader
    wayland
    libxkbcommon
    libGL
    libx11
    libxcb
    libxcursor
    libxi
    libxrandr
  ];
  # What the helper runs. pkexec resets PATH, so their directory is built
  # in (KAKEHASHI_TOOL_PATH in core/src/platform/mod.rs).
  tools = with pkgs; [
    util-linux
    coreutils
    udisks
  ];
in
{
  package = pkgs.rustPlatform.buildRustPackage {
    pname = "kakehashi";
    version = "0.1.0";
    src = lib.fileset.toSource {
      root = source;
      fileset = lib.fileset.unions [
        (source + "/Cargo.toml")
        (source + "/Cargo.lock")
        (source + "/core")
        (source + "/helper")
        (source + "/app")
        (source + "/linux")
      ];
    };
    cargoLock.lockFile = source + "/Cargo.lock";
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.makeWrapper
    ];
    buildInputs = libraries;
    env.KAKEHASHI_TOOL_PATH = lib.makeBinPath tools;
    # Sekka's app icon, from the brand's generated logos.
    postInstall = ''
      install -Dm644 linux/computer.sekka.Kakehashi.desktop -t $out/share/applications
      install -Dm644 ${../docs/brand/logos/sekka-app-icon.svg} \
        $out/share/icons/hicolor/scalable/apps/computer.sekka.Kakehashi.svg
    '';
    # The helper stays unwrapped beside the window, which finds it there.
    postFixup = ''
      wrapProgram $out/bin/kakehashi --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibraries}
    '';
    meta = {
      description = "Write Sekka, or any disk image, to a USB drive and read it back";
      mainProgram = "kakehashi";
      license = with lib.licenses; [
        mit
        asl20
      ];
    };
  };

  shell = pkgs.mkShell {
    packages = [
      pkgs.cargo
      pkgs.rustc
      pkgs.clippy
      pkgs.rustfmt
      pkgs.pkg-config
    ];
    buildInputs = libraries;
    LD_LIBRARY_PATH = lib.makeLibraryPath runtimeLibraries;
    KAKEHASHI_TOOL_PATH = lib.makeBinPath tools;
  };
}
