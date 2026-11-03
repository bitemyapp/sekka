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
in
{
  # The core is a Rust dylib that the binary and every logic library link:
  # scripts/rustc-wrapper gives it `-C prefer-dynamic`. Its build script
  # fingerprints its ABI from the workspace's Cargo.lock and the sources of
  # every path package in its closure (build.rs), so it needs those beside
  # it, laid out as in the workspace, not its own directory alone. Its app
  # icons are Lucide's, included from aizome-kit, which it doesn't depend on.
  aizome-comp-core = attrs: links attrs // {
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
}
