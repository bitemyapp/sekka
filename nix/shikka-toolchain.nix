# The toolchain Shikka builds Aizome's workspace with (DEVELOPERS.md,
# "Building Aizome with Shikka"): `env.json`, which SHIKKA_TOOLCHAIN names.
#
# Shikka compiles each Cargo unit in an environment of its own, never the
# caller's, so everything a unit may use is written down: the compiler, the C
# toolchain, and what stdenv's setup hooks give Aizome's package for its C
# libraries. The libraries are the package's own `buildInputs`
# (desktop/aizome/nix/package.nix), read from its derivation, and so are its
# settings for whisper.cpp: neither is a second list to keep in step. The
# tools its build scripts run by name are listed here; one that is missing
# fails Shikka's build at that script, by name.
#
# Lower-case keys are tools. Upper-case keys become the environment of every
# compilation and build script; BUILD_PATH is their PATH.
{ pkgs, aizome }:
let
  inherit (pkgs) lib;
  # The compiler the package builds with.
  inherit (pkgs.rustPlatform.rust) rustc cargo;
  # Koe's whisper.cpp: CMake, run by whisper-rs-sys, and glslc for its Vulkan
  # shaders.
  scriptTools = [
    pkgs.cmake
    pkgs.shaderc
  ];
  # The package's settings for whisper.cpp and OpenBLAS.
  packageEnvironment = lib.filterAttrs (
    name: value:
    lib.isString value
    && lib.any (prefix: lib.hasPrefix prefix name) [
      "GGML_"
      "BLAS_"
      "OPENBLAS_"
    ]
  ) aizome.drvAttrs;
  tools = {
    rustc = "${rustc}/bin/rustc";
    cargo = "${cargo}/bin/cargo";
    cc = "${pkgs.stdenv.cc}/bin/cc";
    cxx = "${pkgs.stdenv.cc}/bin/c++";
    ar = "${pkgs.stdenv.cc.bintools}/bin/ar";
    # Shikka links the dev profile with mold when it is given one, as
    # Aizome's shell does.
    mold = "${pkgs.mold}/bin/mold";
  };
  # What stdenv's setup hooks give Cargo in the package's build, joined across
  # host and build roles.
  variables = [
    "PKG_CONFIG_PATH"
    "CMAKE_PREFIX_PATH"
    "NIXPKGS_CMAKE_PREFIX_PATH"
    "NIX_CFLAGS_COMPILE"
    "NIX_LDFLAGS"
    "BINDGEN_EXTRA_CLANG_ARGS"
    "LIBCLANG_PATH"
  ];
in
pkgs.stdenv.mkDerivation {
  name = "sekka-shikka-toolchain";
  dontUnpack = true;
  dontConfigure = true;
  dontUseCmakeConfigure = true;
  dontBuild = true;
  dontFixup = true;
  inherit (aizome) buildInputs;
  nativeBuildInputs = scriptTools ++ [
    pkgs.pkg-config
    pkgs.rustPlatform.bindgenHook
    pkgs.jq
  ];
  installPhase = ''
    runHook preInstall
    mkdir -p $out/bin
    ln -s ${lib.getExe pkgs.pkg-config-unwrapped} $out/bin/pkg-config
    # PATH for build scripts that run tools by name (rustc, cc, cmake, glslc),
    # and the libraries' directories for the ones that link test programs
    # (libudev-sys).
    path=$out/bin:${
      lib.makeBinPath (
        [
          rustc
          cargo
          pkgs.stdenv.cc
          pkgs.stdenv.cc.bintools
          pkgs.mold
          pkgs.coreutils
          pkgs.bash
          pkgs.gnumake
        ]
        ++ scriptTools
      )
    }
    libraries=""
    for flag in $NIX_LDFLAGS; do
      case "$flag" in -L/nix/store/*) libraries="$libraries''${libraries:+:}''${flag#-L}" ;; esac
    done
    for name in ${lib.concatStringsSep " " variables}; do
      # PKG_CONFIG_PATH_FOR_TARGET and the like: stdenv names some by role.
      value=""
      for var in "$name" "''${name}_FOR_BUILD" "''${name}_FOR_TARGET"; do
        value="$value ''${!var:-}"
      done
      export "SEKKA_$name=$value"
    done
    jq -n --arg pkg_config "$out/bin/pkg-config" --arg path "$path" --arg libraries "$libraries" '
      def paths: split(" ") | map(select(. != "") | split(":")[]) | map(select(. != "")) | unique | join(":");
      ($ENV | with_entries(select(.key | startswith("SEKKA_")) | .key |= ltrimstr("SEKKA_")))
      | .PKG_CONFIG_PATH |= paths
      | .CMAKE_PREFIX_PATH |= paths
      | .NIXPKGS_CMAKE_PREFIX_PATH |= paths
      | .LIBCLANG_PATH |= (split(" ") | map(select(. != "")) | first // "")
      | map_values(gsub("^ +| +$"; ""))
      | . + {PKG_CONFIG: $pkg_config, BUILD_PATH: $path, LIBRARY_PATH: $libraries}
      | . + $package + $tools
    ' --argjson package ${lib.escapeShellArg (builtins.toJSON packageEnvironment)} \
      --argjson tools ${lib.escapeShellArg (builtins.toJSON tools)} > $out/env.json
    # A hook that stopped setting one of these would otherwise surface as a
    # build script failing far from here.
    jq -e '(.PKG_CONFIG_PATH | length > 0) and (.LIBCLANG_PATH | length > 0) and (.NIX_LDFLAGS | length > 0)
      and (.OPENBLAS_PATH | length > 0) and (.GGML_NATIVE == "OFF")' $out/env.json > /dev/null
    runHook postInstall
  '';
}
