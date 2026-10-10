{ pkgs }:
pkgs.rustPlatform.buildRustPackage {
  pname = "calamares-nixos-rust";
  version = "0.1.0";
  src = pkgs.lib.fileset.toSource {
    root = ../rust;
    fileset = pkgs.lib.fileset.unions [
      ../rust/Cargo.toml
      ../rust/Cargo.lock
      ../rust/data
      ../rust/reference.nix
      ../rust/src
    ];
  };
  cargoLock.lockFile = ../rust/Cargo.lock;
  nativeBuildInputs = [
    pkgs.pkg-config
    pkgs.wrapGAppsHook4
  ];
  buildInputs = [
    pkgs.gtk4
    pkgs.libadwaita
    pkgs.networkmanager
  ];
  # Adwaita symbolic icons and the NixOS logo, independent of the live
  # desktop's icon theme. Referenced store paths stay runtime dependencies.
  CALAMARES_ICON_PATH = "${pkgs.adwaita-icon-theme}/share/icons:${pkgs.nixos-icons}/share/icons";
  CALAMARES_CA_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
  # Privileged children use only this build-time path, never the invoking user's.
  CALAMARES_TOOL_PATH =
    pkgs.lib.makeBinPath [
      pkgs.util-linux
      pkgs.parted
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
  doCheck = true;
  passthru.storageTest = import ./storage-test.nix { inherit pkgs; };
  postInstall = ''
    mkdir -p $out/share/applications $out/share/polkit-1/actions
    cp ${./calamares-nixos.desktop} $out/share/applications/org.calamares.NixOSRust.desktop
    substitute ${./org.calamares.nixos.install.policy.in} \
      $out/share/polkit-1/actions/org.calamares.nixos.install.policy \
      --subst-var-by helper "$out/bin/calamares-nixos-helper"
  '';
  meta = {
    description = "NixOS-focused graphical installer implemented in Rust";
    license = pkgs.lib.licenses.gpl3Plus;
    platforms = [ "x86_64-linux" ];
    mainProgram = "calamares-nixos";
  };
}
