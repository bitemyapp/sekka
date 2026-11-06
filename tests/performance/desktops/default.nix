# The desktop comparison's VMs (docs/performance/desktops.md): installed
# Sekka systems that differ only in the desktop, as the installer's
# `calamares.desktops = [ "<one>" ]` gives them, each with the login screen
# Sekka pairs with it, plus a baseline with no desktop at all.
#
#   nix build --impure -f tests/performance/desktops gnome.toplevel
#
# Evaluated against this checkout's flake (its lock and first-party source),
# so the systems are the ones its own outputs would build.
{
  repo ? toString ../../..,
}:
let
  flake = builtins.getFlake repo;
  system = "x86_64-linux";
  components = import (flake.outPath + "/nix/components.nix") {
    inherit (flake.inputs)
      nixpkgs
      crane
      applications
      superluminal
      tracy
      uta
      ;
  };
  inputs = flake.inputs // components;
  inherit (inputs) nixpkgs;
  inherit (nixpkgs) lib;
  pkgs = nixpkgs.legacyPackages.${system};
  sekka = flake.outPath;
  applicationCatalog = import (flake.outPath + "/nix/applications.nix") { inherit inputs; };
  tools = pkgs.callPackage ./tools.nix { };

  # A desktop as the installer chooses it, with no catalog applications.
  desktopModules = desktop: [
    inputs.calamares.nixosModules.default
    {
      calamares.desktops = [ desktop ];
      calamares.defaultDesktop = desktop;
      calamares.applications = [ ];
      calamares.installUser = "demo";
    }
  ];
  # The same system without a desktop: Sekka's system modules other than
  # desktops.nix (and input-method.nix, which only configures desktops),
  # and the catalog module with nothing chosen. It boots to multi-user.target.
  baselineModules = [
    inputs.calamares.nixosModules.applications
    {
      imports =
        map (f: sekka + "/installer/genkan/rust/system/${f}") [
          "binary-cache.nix"
          "nvidia.nix"
          "prebuilt-binaries.nix"
          "store-space.nix"
          "tuning.nix"
          "usb-wake.nix"
          "wpa-supplicant"
        ]
        ++ map (f: sekka + "/modules/${f}") [
          "portal-fixes.nix"
          "avahi-fixes.nix"
          "pipewire-fixes.nix"
        ];
      environment.systemPackages = [ pkgs.file ];
      calamares.applications = [ ];
      calamares.installUser = "demo";
    }
  ];
  evaluate =
    desktop: instrument:
    lib.nixosSystem {
      inherit system;
      inherit (applicationCatalog) specialArgs;
      modules = [
        ./base.nix
      ]
      ++ (if desktop == null then baselineModules else desktopModules desktop)
      ++ lib.optional instrument (import ./instrument.nix { inherit tools; });
    };
  variant =
    desktop:
    let
      measured = evaluate desktop true;
      plain = evaluate desktop false;
      toplevel = measured.config.system.build.toplevel;
    in
    {
      inherit toplevel;
      config = measured.config;
      # Every store path the VM's erofs store image holds.
      closure = pkgs.closureInfo { rootPaths = [ toplevel ]; };
      # The installed system as it would be, without the benchmark's tools.
      footprint = plain.config.system.build.toplevel;
      versions = pkgs.writeText "versions-${if desktop == null then "baseline" else desktop}.json" (
        builtins.toJSON (
          {
            kernel = measured.config.boot.kernelPackages.kernel.version;
            mesa = pkgs.mesa.version;
            systemd = measured.config.systemd.package.version;
            nixpkgs = nixpkgs.rev or "unknown";
            sekka = flake.rev or flake.dirtyRev or "unknown";
          }
          // lib.optionalAttrs (desktop == "gnome") {
            gnome-shell = pkgs.gnome-shell.version;
            mutter = pkgs.mutter.version;
            gdm = pkgs.gdm.version;
          }
          // lib.optionalAttrs (desktop == "plasma") {
            plasma-workspace = pkgs.kdePackages.plasma-workspace.version;
            kwin = pkgs.kdePackages.kwin.version;
            plasma-login-manager = measured.config.services.displayManager.plasma-login-manager.package.version;
            qt = pkgs.qt6.qtbase.version;
            frameworks = pkgs.kdePackages.kcoreaddons.version;
          }
          // lib.optionalAttrs (desktop == "aizome") {
            aizome = "${measured.config.programs.aizome.package}";
            aizome-version = measured.config.programs.aizome.package.version;
          }
        )
      );
    };
in
{
  inherit tools pkgs;
  baseline = variant null;
  aizome = variant "aizome";
  gnome = variant "gnome";
  plasma = variant "plasma";
  # What the host harness runs: QEMU, the image tools, Python with NumPy.
  host = pkgs.buildEnv {
    name = "desktop-bench-host";
    paths = [
      pkgs.qemu_kvm
      pkgs.erofs-utils
      pkgs.e2fsprogs
      pkgs.util-linux
      pkgs.gnutar
      (pkgs.python3.withPackages (p: [ p.numpy ]))
    ];
  };
}
