# Aizome's outputs, as its own flake gave them before it moved into Sekka:
# its packages, its NixOS module, its VM checks and its development shell,
# built with Sekka's Nixpkgs and Sekka's `sidestep` input
# (nix/components.nix). The checks and their names are the flake's; Sekka
# offers them as `aizome-checks` (`nix build .#aizome-checks.session`), and
# the shell as `nix develop .#aizome`.
{
  nixpkgs,
  sidestep,
  version ? "0.1.0",
  rev ? null,
}:
let
  lib = nixpkgs.lib;
  systems = [
    "x86_64-linux"
    "aarch64-linux"
  ];
  forAll = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
  # Libraries the compositor (smithay), the toolkit (Sidestep) and the
  # apps link against or load at run time.
  buildInputs =
    pkgs: with pkgs; [
      wayland
      libxkbcommon
      libinput
      udev
      seatd
      libdrm
      mesa
      libgbm
      libglvnd
      pixman
      fontconfig
      freetype
      dbus
      pam
      # aizome-sys talks to PipeWire through pipewire-pulse.
      libpulseaudio
      libsecret
    ];
  # Only what the build reads: the workspace and the files the package
  # installs.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
      ../icons
      ../etc
      ../scripts
      (lib.fileset.maybeMissing ../apps)
      (lib.fileset.maybeMissing ../session)
    ];
  };
  self = {
    outPath = ../.;
    # What the checks build their systems with.
    inputs = { inherit nixpkgs sidestep; };

    packages = forAll (pkgs: rec {
      aizome = pkgs.callPackage ./package.nix {
        inherit
          src
          sidestep
          version
          rev
          ;
      };
      archivo = pkgs.callPackage ./fonts.nix { };
      # The session's fonts (Archivo, and nixpkgs' IBM Plex Mono and Noto
      # Sans Symbols 2) as files in one folder, for $AIZOME_FONT_DIR
      # (docs/kit.md).
      fonts = pkgs.callPackage ./aizome-fonts.nix { inherit archivo; };
      default = aizome;
    });

    nixosModules.default = import ./module.nix { inherit self; };
    nixosModules.aizome = self.nixosModules.default;

    # Boots a VM, logs into the Aizome session and checks the desktop is
    # up (vm-test.nix).
    checks = forAll (pkgs: {
      session = import ./vm-test.nix { inherit pkgs self; };
      # Changes something in every pane of Settings and checks the
      # system took it (vm-settings-test.nix).
      settings = import ./vm-settings-test.nix { inherit pkgs self; };
      timezone-policy = import ./timezone-policy-test.nix { inherit pkgs self; };
      # The login screen: people, sessions (another Wayland one, an X11
      # one), a wrong password, logging out and back (vm-greeter-test.nix).
      greeter = import ./vm-greeter-test.nix { inherit pkgs self; };
      greeter-x11 = import ./vm-greeter-x11-test.nix { inherit pkgs self; };
      koyomi-session = import ./vm-koyomi-session-test.nix { inherit pkgs self; };
      koyomi-modal = import ./vm-koyomi-modal-test.nix { inherit pkgs self; };
      shashin-modal = import ./vm-shashin-modal-test.nix { inherit pkgs self; };
      file-panel-multiple = import ./vm-file-panel-multiple-test.nix { inherit pkgs self; };
      terminal-shortcuts = import ./vm-terminal-shortcuts-test.nix { inherit pkgs self; };
      shashin-wallpaper = import ./vm-shashin-wallpaper-test.nix { inherit pkgs self; };
      standard-menu = import ./vm-standard-menu-test.nix { inherit pkgs self; };
      tokei-reorder = import ./vm-tokei-reorder-test.nix { inherit pkgs self; };
      loop-images = import ./vm-loop-images-test.nix { inherit pkgs self; };
      myaku-grouping = import ./vm-myaku-grouping-test.nix { inherit pkgs self; };
      tansu-size = import ./vm-tansu-size-test.nix { inherit pkgs self; };
      tansu-resize = import ./vm-tansu-resize-test.nix { inherit pkgs self; };
      sumi-search = import ./vm-sumi-search-test.nix { inherit pkgs self; };
      fusen-save = import ./vm-fusen-save-test.nix { inherit pkgs self; };
      encrypted-volumes = import ./vm-encrypted-volumes-test.nix { inherit pkgs self; };
      wallpaper-choice = import ./vm-wallpaper-choice-test.nix { inherit pkgs self; };
      network-auth = import ./vm-network-auth-test.nix { inherit pkgs self; };
    });

    devShells = forAll (pkgs: {
      default = pkgs.mkShell {
        nativeBuildInputs = with pkgs; [
          just
          rustc
          cargo
          clippy
          rustfmt
          rust-analyzer
          pkg-config
          clang
          mold
        ];
        buildInputs = buildInputs pkgs;
        # dlopen'd at run time: EGL/GL drivers, fontconfig, xkbcommon.
        LD_LIBRARY_PATH = lib.makeLibraryPath (buildInputs pkgs);
        RUSTFLAGS = "-C link-arg=-fuse-ld=mold";
        # The compositor's core is a Rust dylib that needs -C prefer-dynamic
        # (docs/reload.md); the wrapper adds it for that crate alone.
        RUSTC_WRAPPER = "${../scripts/rustc-wrapper}";
      };
    });
  };
in
self
