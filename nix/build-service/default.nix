{ pkgs }:
let
  # Release archive is independently pinned, not a moving Docker tag. Toolchain
  # identity remains the root flake's native/Rust closure, not this source tree.
  nativelink = pkgs.stdenvNoCC.mkDerivation {
    pname = "nativelink";
    version = "1.8.3";
    src = pkgs.fetchurl {
      url = "https://github.com/TraceMachina/nativelink/releases/download/v1.8.3/nativelink-1.8.3-x86_64-unknown-linux-musl.tar.gz";
      hash = "sha256-GmWWyW8FnU6Z9zUhZDIeHxzf9DUnVmethInoP34pd8U=";
    };
    sourceRoot = ".";
    installPhase = ''
      runHook preInstall
      install -Dm755 $(find . -type f -name nativelink) $out/bin/nativelink
      runHook postInstall
    '';
    meta.platforms = [ "x86_64-linux" ];
  };
  controller = pkgs.writeShellApplication {
    name = "sekka-build-service";
    runtimeInputs = [ pkgs.python3 pkgs.openssh ];
    text = ''
      export SEKKA_BUILD_SERVICE_SHELL=${pkgs.bashInteractive}/bin/bash
      export SEKKA_BUILD_SERVICE_ENV=${pkgs.coreutils}/bin/env
      exec python3 ${../../scripts/build-service} "$@"
    '';
  };
  qualificationPython = pkgs.python3.withPackages (p: [ p.grpcio ]);
  qualifier = pkgs.writeShellApplication {
    name = "sekka-build-service-qualify";
    text = ''exec ${qualificationPython}/bin/python3 ${../../scripts/build-service-qualify} "$@"'';
  };
in
{
  inherit nativelink controller qualifier qualificationPython;
}
