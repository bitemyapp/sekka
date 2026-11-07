# Sekka's packages for Apple Silicon Macs (aarch64-linux): its own components,
# built as they are for PCs, and the platform Omarchy's release
# aurora-2026.10.17.20 was reported working with on a 14-inch M3 Pro (J514S):
# the linux-aurora kernel, m1n1-aurora and the M3 Pro's Mesa. module.nix puts
# them together for installed systems (calamares.appleSilicon).
{
  inputs,
  nixpkgs,
  hyprlandOverlay,
  # Sekka's own flake: its source and revision pin the installed flake.lock
  # the Mac images ship (installed-flake.nix).
  self,
  # Sekka's revision, which names the Asahi installer package.
  revision,
}:
let
  image = import ./image.nix {
    inherit
      inputs
      nixpkgs
      self
      revision
      ;
  };
  tests = import ./tests.nix {
    inherit pkgs self image;
    inherit (nixpkgs) lib;
  };
  system = "aarch64-linux";
  pkgs = nixpkgs.legacyPackages.${system};
  hyprlandPkgs = pkgs.extend hyprlandOverlay;
  kernel = pkgs.callPackage ./kernel.nix { };
  mesa = pkgs.callPackage ./mesa.nix { };
  graphics = pkgs.callPackage ./graphics.nix { inherit mesa; };

  # An installed Tatami system for the 14-inch MacBook Pro with an M3 Pro, from
  # the same reference configuration the installation media prebuild for PCs
  # (installer/genkan/rust/reference.nix), with the handoff on.
  j514s = nixpkgs.lib.nixosSystem {
    inherit system;
    specialArgs = {
      applicationPkgs = import inputs.applications {
        inherit system;
        config.allowUnfree = true;
      };
      aiPackages = inputs.ai-apps.packages.${system};
      ompPackage = inputs.omp.packages.${system}.omp;
    };
    modules = [
      inputs.determinate.nixosModules.default
      (import ../../installer/genkan/rust/reference.nix {
        desktops = [
          "aizome"
          "tatami"
        ];
        kernel = "linux-aurora";
      })
      # The reference lists a PC's storage drivers, ahci among them, which
      # this kernel lacks; a Mac's own hardware configuration wouldn't.
      (
        { config, lib, ... }:
        {
          boot.initrd.availableKernelModules = lib.mkForce config.calamares.appleSilicon.initrdModules;
        }
      )
      inputs.aizome.nixosModules.default
      inputs.tatami.nixosModules.default
      inputs.yukimi.nixosModules.default
      { _module.args.sekkaPackages = inputs.catalog.packages.${system}; }
      {
        calamares.appleSilicon = {
          enable = true;
          t6030Handoff = true;
        };
      }
    ];
  };
in
{
  genkan = import ../calamares.nix {
    inherit pkgs;
    source = inputs.calamares;
    fonts = inputs.aizome.packages.${system}.fonts;
  };
  inherit (inputs.aizome.packages.${system}) aizome;
  aizome-fonts = inputs.aizome.packages.${system}.fonts;
  tools = import ../tools.nix { inherit pkgs; };
  inherit (inputs.tatami.packages.${system})
    tatami
    displays
    network
    activity
    wallpapers
    ;
  inherit (inputs.yukimi.packages.${system}) yukimi;
  inherit (inputs.kakehashi.packages.${system}) kakehashi;
  inherit (hyprlandPkgs) aquamarine hyprland;

  linux-aurora = kernel.kernel;
  m1n1-aurora = pkgs.callPackage ./m1n1.nix { };
  uboot-asahi = pkgs.callPackage ./uboot.nix { };
  mesa-m3-native-pro = mesa.native;
  mesa-m3-pro-vulkan = mesa.vulkan;
  mesa-m3-abi-check = graphics.abiCheck;
  m3-gpu-session-env-test = graphics.sessionEnvTest;

  # The developer image: the Asahi installer's OS package for the J514S, with
  # installer_data.json and install-sekka.sh (image.nix).
  mac-installer-j514s = image.bundle;
  image-j514s-system = image.developer.toplevel;
  image-j514s-esp = image.developer.esp;
  # The product image's release set for Sekka's macOS installer: payload,
  # installer data, manifest (sekka.mac.manifest/1) and /etc/nixos.
  mac-release-j514s = image.release;
  mac-release-j514s-system = image.product.toplevel;
  mac-release-j514s-esp = image.product.esp;
  mac-release-j514s-root = image.product.rootImage;
  mac-release-j514s-etc-nixos = image.product.etcNixos;
  mac-release-j514s-image-check = image.product.imageCheck;
  mac-release-j514s-rebuild-inputs = image.product.rebuildInputs;
  # The image's scripts and its installed flake against Genkan's own
  # template parser (tests.nix).
  mac-image-tests = tests.all;

  installed-j514s = j514s.config.system.build.toplevel;
  installed-j514s-boot-bin = j514s.config.system.build.appleSiliconBootBin;
}
