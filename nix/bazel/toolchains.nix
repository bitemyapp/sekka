# What Bazel builds Sekka's Rust with (docs/bazel-build-plan.md), from the root
# flake's Nixpkgs: Bazel's repository rules build these attributes with
# `<nixpkgs>` from nix/bazel/nixpkgs.nix (build/bazel/nix.bzl), and the flake's
# `bazel` shell with its own `pkgs`. Same lock, same store paths.
#
# - `rust`: the compiler, Cargo, Clippy, rustfmt and rust-analyzer the Aizome
#   shell and package use, with the BUILD file of their Bazel toolchain.
# - `cc`: stdenv's C/C++ compiler and binutils, which rules_nixpkgs_cc
#   configures as Bazel's C/C++ toolchain (and rules_rust links with).
# - `native`: the environment Aizome's package builds in (its buildInputs, as
#   stdenv's setup hooks see them), for the build scripts of crates that find
#   C libraries with pkg-config or generate bindings (build/bazel/native.bzl).
{
  pkgs ? import <nixpkgs> { },
}:
let
  inherit (pkgs) lib;
  target = pkgs.stdenv.hostPlatform.rust.rustcTarget;
  aizome = import ./aizome-inputs.nix { inherit pkgs; };
  # rules_rust passes each as -C opt-level and -C debuginfo, by compilation
  # mode. Cargo's profiles (desktop/aizome/Cargo.toml): `dev` (fastbuild)
  # and `release` (opt) keep line tables; .bazelrc's `dev` config builds
  # dependencies with opt-level 2 and no debug information, as Cargo does.
  rustBuild = ''
    filegroup(name = "rustc", srcs = ["bin/rustc"], visibility = ["//visibility:public"])
    filegroup(name = "rustfmt", srcs = ["bin/rustfmt"], visibility = ["//visibility:public"])
    filegroup(name = "cargo", srcs = ["bin/cargo"], visibility = ["//visibility:public"])
    filegroup(name = "clippy_driver", srcs = ["bin/clippy-driver"], visibility = ["//visibility:public"])
    filegroup(name = "rust_doc", srcs = ["bin/rustdoc"], visibility = ["//visibility:public"])
    filegroup(name = "rust_analyzer", srcs = ["bin/rust-analyzer"], visibility = ["//visibility:public"])
    filegroup(
        name = "rust_analyzer_proc_macro_srv",
        srcs = ["libexec/rust-analyzer-proc-macro-srv"],
        visibility = ["//visibility:public"],
    )
    filegroup(
        name = "rustc_srcs",
        srcs = glob(["lib/rustlib/src/rust/library/**"]),
        visibility = ["//visibility:public"],
    )
    filegroup(
        name = "rustc_lib",
        srcs = glob(
            [
                "lib/*.so",
                "lib/rustlib/*/codegen-backends/*.so",
                "lib/rustlib/*/bin/rust-lld",
                "lib/rustlib/*/lib/*.so",
            ],
            allow_empty = True,
        ),
        visibility = ["//visibility:public"],
    )

    load("@rules_rust//rust:toolchain.bzl", "rust_analyzer_toolchain", "rust_stdlib_filegroup", "rust_toolchain", "rustfmt_toolchain")

    rust_stdlib_filegroup(
        name = "rust_std",
        srcs = glob(
            [
                "lib/rustlib/${target}/lib/*.rlib",
                "lib/rustlib/${target}/lib/*.rmeta",
                "lib/rustlib/${target}/lib/*.so",
                "lib/rustlib/${target}/lib/*.a",
            ],
            allow_empty = True,
        ),
        visibility = ["//visibility:public"],
    )

    rust_toolchain(
        name = "rust_nix_impl",
        rust_doc = ":rust_doc",
        rust_std = ":rust_std",
        rustc = ":rustc",
        rustfmt = ":rustfmt",
        cargo = ":cargo",
        clippy_driver = ":clippy_driver",
        rustc_lib = ":rustc_lib",
        binary_ext = "",
        staticlib_ext = ".a",
        dylib_ext = ".so",
        exec_triple = "${target}",
        target_triple = "${target}",
        default_edition = "2024",
        stdlib_linkflags = ["-lpthread", "-ldl"],
        opt_level = {"dbg": "0", "fastbuild": "0", "opt": "3"},
        debug_info = {"dbg": "2", "fastbuild": "line-tables-only", "opt": "line-tables-only"},
        strip_level = {"dbg": "none", "fastbuild": "none", "opt": "none"},
        visibility = ["//visibility:public"],
    )

    rustfmt_toolchain(name = "rustfmt_nix_impl", rustfmt = ":rustfmt", visibility = ["//visibility:public"])

    rust_analyzer_toolchain(
        name = "rust_analyzer_nix_impl",
        proc_macro_srv = ":rust_analyzer_proc_macro_srv",
        rustc = ":rustc",
        rustc_srcs = ":rustc_srcs",
        visibility = ["//visibility:public"],
    )
  '';
  # What the native environment exposes (`$(NAME)` in a build script's
  # environment, build/bazel/native.bzl): the variables stdenv's setup hooks
  # give Cargo in the package's build, joined across host and build roles.
  nativeVariables = [
    "PKG_CONFIG_PATH"
    "CMAKE_PREFIX_PATH"
    "NIXPKGS_CMAKE_PREFIX_PATH"
    "NIX_CFLAGS_COMPILE"
    "NIX_LDFLAGS"
    "BINDGEN_EXTRA_CLANG_ARGS"
    "LIBCLANG_PATH"
  ];
in
rec {
  # As rules_nixpkgs_cc assembles stdenv's compiler by default, less gold:
  # it links with -fuse-ld=gold when it finds ld.gold, and the Aizome
  # package links with GNU ld. With mold, which the Aizome shell links with
  # (.bazelrc's `dev`).
  cc = pkgs.buildEnv {
    name = "sekka-bazel-cc";
    paths = [
      pkgs.stdenv.cc
      pkgs.stdenv.cc.cc
      pkgs.stdenv.cc.bintools
      pkgs.mold
    ];
    pathsToLink = [ "/bin" ];
    postBuild = ''
      rm -f $out/bin/ld.gold $out/bin/*-ld.gold
      # buildEnv also includes the underlying compiler for its tools. Ensure
      # C++ aliases never win a collision against stdenv's wrapped compiler:
      # the wrapper supplies the libc startup objects and Nix linker flags.
      for tool in cc c++ gcc g++ cpp; do
        if test -e ${pkgs.stdenv.cc}/bin/$tool; then
          ln -sfn ${pkgs.stdenv.cc}/bin/$tool $out/bin/$tool
        fi
      done
    '';
    passthru = {
      inherit (pkgs.stdenv.cc) isClang targetPrefix;
      orignalName = pkgs.stdenv.cc.name;
    };
  };

  rust = pkgs.buildEnv {
    name = "sekka-bazel-rust-toolchain";
    paths = [
      pkgs.rustc
      pkgs.rustc.unwrapped
      pkgs.cargo
      pkgs.clippy
      pkgs.rustfmt
      pkgs.rust-analyzer
      # The standard library's source, where rust-analyzer looks for it.
      (pkgs.runCommand "rust-lib-src" { } ''
        mkdir -p $out/lib/rustlib/src/rust
        ln -s ${pkgs.rustPlatform.rustLibSrc} $out/lib/rustlib/src/rust/library
      '')
    ];
    extraOutputsToInstall = [ "lib" ];
    ignoreCollisions = true;
    postBuild = ''
      touch $out/REPO.bazel
      cat > $out/BUILD.bazel <<'EOF'
      ${rustBuild}
      EOF
    '';
  };

  native = pkgs.stdenv.mkDerivation {
    name = "sekka-bazel-native-env";
    dontUnpack = true;
    dontConfigure = true;
    dontUseCmakeConfigure = true;
    dontBuild = true;
    inherit (aizome) buildInputs;
    nativeBuildInputs = aizome.nativeBuildInputs ++ [
      pkgs.pkg-config
      pkgs.rustPlatform.bindgenHook
      pkgs.jq
    ];
    installPhase = ''
      runHook preInstall
      mkdir -p $out/bin
      touch $out/REPO.bazel
      ln -s ${lib.getExe pkgs.pkg-config-unwrapped} $out/bin/pkg-config
      # PATH for build scripts that run tools by name (rustc, cc, cmake,
      # glslc), and the libraries' directories for the ones that link test
      # programs (libudev-sys).
      tools=$out/bin:${
        lib.makeBinPath (
          [
            rust
            cc
            pkgs.coreutils
            pkgs.bash
            pkgs.gnumake
          ]
          ++ aizome.nativeBuildInputs
        )
      }
      libraries=""
      for flag in $NIX_LDFLAGS; do
        case "$flag" in -L/nix/store/*) libraries="$libraries''${libraries:+:}''${flag#-L}" ;; esac
      done
      for name in ${lib.concatStringsSep " " nativeVariables}; do
        # PKG_CONFIG_PATH_FOR_TARGET and the like: stdenv names some by role.
        value=""
        for var in "$name" "''${name}_FOR_BUILD" "''${name}_FOR_TARGET"; do
          value="$value ''${!var:-}"
        done
        export "SEKKA_$name=$value"
      done
      jq -n --arg pkg_config "$out/bin/pkg-config" --arg tools "$tools" --arg libraries "$libraries" '
        ($ENV | with_entries(select(.key | startswith("SEKKA_")) | .key |= ltrimstr("SEKKA_")))
        | .PKG_CONFIG_PATH |= (split(" ") | map(select(. != "") | split(":")[]) | map(select(. != "")) | unique | join(":"))
        | .CMAKE_PREFIX_PATH |= (split(" ") | map(select(. != "") | split(":")[]) | map(select(. != "")) | unique | join(":"))
        | .NIXPKGS_CMAKE_PREFIX_PATH |= (split(" ") | map(select(. != "") | split(":")[]) | map(select(. != "")) | unique | join(":"))
        | .LIBCLANG_PATH |= (split(" ") | map(select(. != "")) | first // "")
        | map_values(if type == "string" then gsub("^ +| +$"; "") else . end)
        | . + {PKG_CONFIG: $pkg_config, BUILD_PATH: $tools, LIBRARY_PATH: $libraries}
        | . + $package
      ' --argjson package ${lib.escapeShellArg (builtins.toJSON aizome.env)} > $out/env.json
      {
        echo '"""Aizome'"'"'s native build environment, from nix/bazel/toolchains.nix."""'
        echo
        echo "NATIVE_ENV = $(jq -S . $out/env.json)"
      } > $out/env.bzl
      # The libraries' directories, which every link sees in the Nix build
      # (NIX_LDFLAGS): build/bazel/generate.py gives them to every crate.
      {
        echo 'load("@rules_cc//cc:defs.bzl", "cc_library")'
        echo
        echo 'exports_files(["env.bzl", "env.json"])'
        echo
        echo 'cc_library('
        echo '    name = "link_search",'
        echo '    linkopts = ['
        for flag in $NIX_LDFLAGS; do
          case "$flag" in -L/nix/store/*) echo "        \"$flag\"," ;; esac
        done
        echo '    ],'
        echo '    visibility = ["//visibility:public"],'
        echo ')'
      } > $out/BUILD.bazel
      runHook postInstall
    '';
  };

  # What Aizome's package gives its tests (the checkPhase of
  # desktop/aizome/nix/package.nix), for Bazel's (build/bazel/native.bzl):
  # the session's fonts and Hotaru's, fontconfig with the CJK fallback, time
  # zones, the libraries the kit and smithay open at run time, and dbus-daemon
  # for Hako's private bus. Apart from `native`, so they never invalidate a
  # compilation.
  testEnv =
    let
      archivo = pkgs.callPackage ../../desktop/aizome/nix/fonts.nix { };
      fonts = pkgs.callPackage ../../desktop/aizome/nix/aizome-fonts.nix { inherit archivo; };
      hotaruFonts = pkgs.callPackage ../../desktop/aizome/nix/hotaru-fonts.nix { };
      variables = {
        AIZOME_FONT_DIR = "${fonts}";
        HOTARU_FONT_DIR = "${hotaruFonts}";
        FONTCONFIG_FILE = "${pkgs.makeFontsConf {
          fontDirectories = [
            fonts
            pkgs.noto-fonts-cjk-sans
          ];
        }}";
        TZDIR = "${pkgs.tzdata}/share/zoneinfo";
        LD_LIBRARY_PATH = lib.makeLibraryPath [
          pkgs.libglvnd
          pkgs.libpulseaudio
          (import ../libsecret-dh-padding.nix { inherit (pkgs) lib libsecret; })
          pkgs.fontconfig
          pkgs.libxkbcommon
          pkgs.wayland
        ];
        # stdenv's tools too: Bazel's test runner (generate-xml.sh) shares it.
        PATH = lib.makeBinPath [
          pkgs.dbus
          pkgs.systemd
          pkgs.wl-clipboard
          pkgs.dconf
          pkgs.ffmpeg
          pkgs.bash
          pkgs.coreutils
          pkgs.findutils
          pkgs.diffutils
          pkgs.gnugrep
          pkgs.gnused
          pkgs.gawk
        ];
      };
    in
    pkgs.runCommand "sekka-bazel-test-env" { } ''
      mkdir $out
      touch $out/REPO.bazel
      cat > $out/env.json <<'EOF'
      ${builtins.toJSON variables}
      EOF
      {
        echo '"""Aizome'"'"'s test environment, from nix/bazel/toolchains.nix."""'
        echo
        echo 'TEST_ENV = ${builtins.toJSON variables}'
      } > $out/env.bzl
      echo 'exports_files(["env.bzl", "env.json"])' > $out/BUILD.bazel
    '';
}
