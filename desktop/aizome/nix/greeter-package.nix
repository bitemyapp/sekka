# The login screen alone (aizome-greeter), built from this workspace as the
# package builds it: for VM checks that run the greeter of a checkout with
# another Aizome build's compositor and session (installer/genkan/
# first-boot's), so they needn't wait for the whole desktop to rebuild. The
# package proper carries the same greeter.
{
  lib,
  stdenv,
  aizome,
  libglvnd,
  libpulseaudio,
  libsecret,
  fontconfig,
  libxkbcommon,
  wayland,
}:
let
  target = stdenv.hostPlatform.rust.rustcTarget;
in
aizome.overrideAttrs (_: {
  pname = "aizome-greeter";
  outputs = [ "out" ];
  buildPhase = ''
    runHook preBuild
    cargo build -j "$NIX_BUILD_CORES" --target ${target} --offline --release -p aizome-greeter
    runHook postBuild
  '';
  doCheck = false;
  installPhase = ''
    runHook preInstall
    install -Dm755 target/${target}/release/aizome-greeter $out/bin/aizome-greeter
    runHook postInstall
  '';
  # What the kit opens at run time, as the package's binaries have it.
  postFixup = ''
    patchelf --add-rpath ${
      lib.makeLibraryPath [
        libglvnd
        libpulseaudio
        libsecret
        fontconfig
        libxkbcommon
        wayland
      ]
    } $out/bin/aizome-greeter
  '';
  passthru = { };
  meta = {
    description = "Aizome's login screen alone, for VM checks";
    mainProgram = "aizome-greeter";
    platforms = lib.platforms.linux;
  };
})
