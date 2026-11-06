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
  # The backend at its place in Sekka, beside the typestate support its path
  # dependency names (Aizome's crate, which has no dependencies).
  backend = lib.fileset.toSource {
    root = ../../..;
    fileset = lib.fileset.unions [
      ../rust/Cargo.toml
      ../rust/Cargo.lock
      # The program ban the helper's check runs.
      ../rust/clippy.toml
      ../rust/data
      ../rust/reference.nix
      ../rust/src
      ../../../desktop/aizome/crates/aizome-typestate
    ];
  };
  helper = pkgs.rustPlatform.buildRustPackage (
    env
    // {
      pname = "calamares-nixos-helper";
      version = "0.1.0";
      src = backend;
      postUnpack = ''
        sourceRoot="$sourceRoot/installer/genkan/rust"
      '';
      cargoLock.lockFile = ../rust/Cargo.lock;
      nativeBuildInputs = [
        pkgs.pkg-config
        pkgs.clippy
      ];
      # The installer runs programs only through its library, which names
      # them; rust/clippy.toml denies the other ways to start one. Only that
      # lint fails the build, so a toolchain's new lints don't.
      #
      # Unsafe code and that ban may be allowed only where the crate says:
      # unsafe_code in sys.rs (system calls) and wifi/nm.rs (libnm's FFI),
      # the ban in process.rs (where programs start). The check takes every
      # line under src/ that names either lint outside a // comment (any
      # attribute, cfg_attr or multi-line one included), drops its line
      # number, and fails unless the sorted lines are exactly these.
      #
      # The default check's `cargo test` runs the doctests too, their
      # compile_fail error codes checked.
      preCheck = ''
        source ${../../../nix/checked-doctests.sh}
        # Without its configuration the ban names nothing, and passes.
        if [ ! -f clippy.toml ]; then
          echo "rust/clippy.toml isn't in the helper's source, so the program ban would check nothing"
          exit 1
        fi
        cargo clippy --offline --locked --all-targets -- \
          -A clippy::all -D clippy::disallowed_methods
        found=$(grep -rnE --include='*.rs' '(unsafe_code|disallowed_methods)' src \
          | grep -vE '^[^:]+:[0-9]+:\s*//' | sed -E 's/:[0-9]+:\s*/: /' | sort)
        expected=$(printf '%s\n' \
          'src/helper.rs: #![forbid(clippy::disallowed_methods)]' \
          'src/helper.rs: #![forbid(unsafe_code)]' \
          'src/lib.rs: #![deny(clippy::disallowed_methods)]' \
          'src/lib.rs: #![deny(unsafe_code)]' \
          'src/process.rs: #[allow(clippy::disallowed_methods)]' \
          'src/process.rs: #[allow(clippy::disallowed_methods)]' \
          'src/process.rs: #[allow(clippy::disallowed_methods)]' \
          'src/sys.rs: #![allow(unsafe_code)]' \
          'src/wifi/nm.rs: #![allow(unsafe_code)]' | sort)
        if [ "$found" != "$expected" ]; then
          echo "unsafe_code or the program ban is allowed where it may not be:"
          diff <(echo "$expected") <(echo "$found") || true
          exit 1
        fi
      '';
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
      # The typestate support the window and the backend share.
      (aizome + "/crates/aizome-typestate")
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
    cp -r ${backend}/installer/genkan/rust $out/installer/genkan/rust
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
    # The window's tests draw its pages offscreen with Aizome's fonts. Its
    # doctests' compile_fail error codes are checked.
    doCheck = true;
    preCheck = ''
      source ${../../../nix/checked-doctests.sh}
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
