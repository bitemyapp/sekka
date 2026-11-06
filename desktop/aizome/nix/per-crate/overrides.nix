# Per-crate additions to nixpkgs' defaultCrateOverrides: what each crate's
# build needs that Cargo would find on its own in ../package.nix's single
# build. Bazel's crate_universe needs the same facts as annotations.
{ pkgs, crates }:
let
  inherit (pkgs) lib;
  # The repository, and the workspace in it, as the compositor's core sees
  # them from crates/<name> (some path packages live in Sekka's apps/).
  repository = ../../../..;
  workspace = ../..;
  local = id: crates.${id}.src._isLibCleanSourceWith or false;
  # Every path package Cargo.lock puts in a crate's closure (any kind of
  # dependency), as the core's build script walks it.
  pathClosure =
    root:
    builtins.filter local (
      map (c: c.key) (
        builtins.genericClosure {
          startSet = [ { key = root; } ];
          operator =
            { key }:
            map (d: { key = d.packageId; }) (
              (crates.${key}.dependencies or [ ])
              ++ (crates.${key}.buildDependencies or [ ])
              ++ (crates.${key}.devDependencies or [ ])
            );
        }
      )
    );
  # The system libraries smithay and the rest link, by name and without a
  # search path of their own (`#[link(name = "input")]`): Cargo's one build
  # has them all as buildInputs, but here only the crates that link a
  # library or binary see them, so those need the whole set.
  linkInputs = with pkgs; [
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
  links = attrs: { buildInputs = (attrs.buildInputs or [ ]) ++ linkInputs; };
  # Build scripts that generate bindings: libclang (bindgenHook), pkg-config
  # and the headers of the library bound.
  bindgen = inputs: attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [
      pkgs.pkg-config
      pkgs.rustPlatform.bindgenHook
    ];
    buildInputs = (attrs.buildInputs or [ ]) ++ inputs;
  };
in
{
  # The core is a Rust dylib that the binary and every logic library link:
  # scripts/rustc-wrapper gives it `-C prefer-dynamic`. Its build script
  # fingerprints its ABI from the workspace's Cargo.lock and the sources of
  # every path package in its closure (build.rs), so it needs those beside
  # it, laid out as in the workspace, not its own directory alone. Its app
  # icons are Lucide's, included from aizome-kit, which it doesn't depend on.
  aizome-comp-core =
    attrs:
    links attrs
    // {
      src = lib.fileset.toSource {
        root = repository;
        fileset = lib.fileset.unions (
          [
            (workspace + "/Cargo.toml")
            (workspace + "/Cargo.lock")
            (workspace + "/crates/aizome-kit/icons/lucide")
          ]
          ++ map (id: crates.${id}.src.origSrc) (pathClosure "aizome-comp-core")
        );
      };
      workspace_member = "desktop/aizome/crates/aizome-comp-core";
      extraRustcOpts = (attrs.extraRustcOpts or [ ]) ++ [ "-C prefer-dynamic" ];
    };
  aizome-comp = links;
  aizome-comp-logic = links;

  # Outside the compositor: the crates with the heaviest build scripts,
  # for Koe (speech recognition) and aizome-media (Utsushi's playback).
  ffmpeg-sys-next = bindgen [ pkgs.ffmpeg ];
  libspa-sys = bindgen [ pkgs.pipewire ];
  pipewire-sys = bindgen [ pkgs.pipewire ];
  # libspa's build script also probes pkg-config (system-deps) and compiles
  # a C test helper against the headers.
  libspa = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkgs.pkg-config ];
    buildInputs = (attrs.buildInputs or [ ]) ++ [ pkgs.pipewire ];
  };
  pipewire = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkgs.pkg-config ];
    buildInputs = (attrs.buildInputs or [ ]) ++ [ pkgs.pipewire ];
  };
  # Cargo hands a `links` crate's build-script metadata to its dependents'
  # build scripts (DEP_WHISPER_*); buildRustCrate doesn't, so whisper-rs
  # gets whisper-rs-sys's one key by hand. It has to match the whisper.cpp
  # that whisper-rs-sys vendors.
  whisper-rs = attrs: { DEP_WHISPER_WHISPER_CPP_VERSION = "1.8.3"; };
  libpulse-sys = attrs: {
    nativeBuildInputs = (attrs.nativeBuildInputs or [ ]) ++ [ pkgs.pkg-config ];
    buildInputs = (attrs.buildInputs or [ ]) ++ [ pkgs.libpulseaudio ];
  };
  # whisper.cpp through CMake, with OpenBLAS and Vulkan, as ../package.nix
  # builds it (see its comments), including the stack limit for the shader
  # generator's threads.
  whisper-rs-sys =
    attrs:
    let
      b = bindgen (with pkgs; [
        openblas
        vulkan-headers
        vulkan-loader
      ]) attrs;
    in
    b
    // {
      nativeBuildInputs = b.nativeBuildInputs ++ [
        pkgs.cmake
        pkgs.shaderc
      ];
      dontUseCmakeConfigure = true;
      preConfigure = (attrs.preConfigure or "") + ''
        ulimit -Ss 8192
      '';
      BLAS_INCLUDE_DIRS = "${lib.getDev pkgs.openblas}/include";
      OPENBLAS_PATH = "${lib.getLib pkgs.openblas}";
      GGML_NATIVE = "OFF";
      GGML_AVX = "OFF";
      GGML_AVX2 = "OFF";
      GGML_FMA = "OFF";
      GGML_F16C = "OFF";
    };
  aizome-koe =
    attrs:
    let
      l = links attrs;
    in
    l
    // {
      buildInputs = l.buildInputs ++ [
        pkgs.openblas
        pkgs.vulkan-loader
      ];
    };
}
