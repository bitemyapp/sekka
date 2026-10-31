# Genkan: its window (ui/, on Aizome's kit and Sukumo) and its privileged
# helper (rust/), from two Cargo workspaces with their own locks. The
# helper is built from the backend alone; the window links the same
# backend, Aizome's kit crates and Sukumo, and the package puts the
# helper beside it, where the window and the Polkit policy look for it.
{
  pkgs,
  fonts,
}:
let
  inherit (pkgs) lib;
  # What the backend reads at build time, in the window and the helper alike.
  env = {
    CALAMARES_CA_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
    # Privileged children use only this build-time path, never the invoking user's.
    CALAMARES_TOOL_PATH =
      lib.makeBinPath [
        pkgs.util-linux
        pkgs.parted
        pkgs.cryptsetup
        pkgs.e2fsprogs
        pkgs.btrfs-progs
        pkgs.xfsprogs
        pkgs.dosfstools
        pkgs.kmod
        pkgs.systemd
        pkgs.coreutils
        pkgs.curl
      ]
      + ":/run/current-system/sw/bin";
    # NixOS installs the setuid entry point here; the store binary is not setuid.
    CALAMARES_PKEXEC = "/run/wrappers/bin/pkexec";
  };
  backend = lib.fileset.toSource {
    root = ../rust;
    fileset = lib.fileset.unions [
      ../rust/Cargo.toml
      ../rust/Cargo.lock
      ../rust/data
      ../rust/reference.nix
      ../rust/src
    ];
  };
  helper = pkgs.rustPlatform.buildRustPackage (
    env
    // {
      pname = "calamares-nixos-helper";
      version = "0.1.0";
      src = backend;
      cargoLock.lockFile = ../rust/Cargo.lock;
      nativeBuildInputs = [ pkgs.pkg-config ];
      buildInputs = [
        pkgs.glib
        pkgs.networkmanager
      ];
      cargoBuildFlags = [
        "--bin"
        "calamares-nixos-helper"
      ];
      doCheck = true;
    }
  );
  # Only the kit and what it stands on from Aizome's workspace, with the
  # workspace's manifest for the versions they inherit: Aizome's apps,
  # compositor and shell don't rebuild Genkan, and Genkan's changes don't
  # rebuild Aizome.
  aizome = ../../../desktop/aizome;
  kit = lib.fileset.toSource {
    root = aizome;
    fileset = lib.fileset.unions [
      (aizome + "/Cargo.toml")
      (aizome + "/crates/aizome-kit")
      (aizome + "/crates/aizome-theme")
      (aizome + "/crates/aizome-config")
      (aizome + "/crates/aizome-shell-proto")
      # The compositor's IPC, for the keyboard step's key.
      (aizome + "/crates/aizome-ipc")
      # Sukumo, which the kit draws with: its workspace manifest and crates.
      (aizome + "/sukumo/Cargo.toml")
      (aizome + "/sukumo/crates")
    ];
  };
  ui = lib.fileset.toSource {
    root = ../ui;
    fileset = lib.fileset.unions [
      ../ui/Cargo.toml
      ../ui/Cargo.lock
      ../ui/src
      ../ui/tests
    ];
  };
  # Sekka's paths, as the crates' path dependencies name them.
  source = pkgs.runCommand "genkan-source" { } ''
    mkdir -p $out/installer/genkan $out/desktop
    cp -r ${ui} $out/installer/genkan/ui
    cp -r ${backend} $out/installer/genkan/rust
    cp -r ${kit} $out/desktop/aizome
  '';
  # Opened at run time rather than linked (Sukumo's text and windows).
  runtimeLibs = [
    pkgs.fontconfig
    pkgs.libxkbcommon
    pkgs.wayland
  ];
in
pkgs.rustPlatform.buildRustPackage (
  env
  // {
    pname = "genkan";
    version = "0.1.0";
    src = source;
    sourceRoot = "genkan-source/installer/genkan/ui";
    cargoLock.lockFile = ../ui/Cargo.lock;
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [
      pkgs.glib
      pkgs.networkmanager
      pkgs.fontconfig
      pkgs.freetype
      pkgs.libxkbcommon
      pkgs.wayland
    ];
    # The window's tests draw its pages offscreen with Aizome's fonts.
    doCheck = true;
    preCheck = ''
      export AIZOME_FONT_DIR=${fonts}
      export TZDIR=${pkgs.tzdata}/share/zoneinfo
    '';
    passthru = {
      inherit helper;
      storageTest = import ./storage-test.nix { inherit pkgs; };
    };
    postInstall = ''
      install -m755 ${helper}/bin/calamares-nixos-helper $out/bin/calamares-nixos-helper
      install -Dm644 ${./calamares-nixos.desktop} $out/share/applications/org.calamares.NixOSRust.desktop
      mkdir -p $out/share/polkit-1/actions
      substitute ${./org.calamares.nixos.install.policy.in} \
        $out/share/polkit-1/actions/org.calamares.nixos.install.policy \
        --subst-var-by helper "$out/bin/calamares-nixos-helper"
    '';
    postFixup = ''
      patchelf --add-rpath ${lib.makeLibraryPath runtimeLibs} $out/bin/calamares-nixos
    '';
    meta = {
      description = "Genkan, Sekka's installer, with its window on Aizome's kit";
      license = lib.licenses.gpl3Plus;
      platforms = [ "x86_64-linux" ];
      mainProgram = "calamares-nixos";
    };
  }
)
