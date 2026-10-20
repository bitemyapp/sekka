# Native ARM components and focused installation media for UTM on Apple silicon.
{
  inputs,
  installedLock,
  sekkaSource,
}:
let
  system = "aarch64-linux";
  inherit (inputs.nixpkgs) lib;
  pkgs = inputs.nixpkgs.legacyPackages.${system};
  genkan = import ./calamares.nix {
    inherit pkgs;
    source = inputs.calamares;
  };
  installer = lib.nixosSystem {
    inherit system;
    specialArgs = { inherit inputs installedLock sekkaSource; };
    modules = [
      inputs.determinate.nixosModules.default
      "${inputs.nixpkgs}/nixos/modules/installer/cd-dvd/installation-cd-graphical-base.nix"
      ../modules/installer.nix
      ../modules/utm.nix
      ({ lib, pkgs, ... }: {
        environment.systemPackages = [
          pkgs.mesa-demos
          pkgs.glmark2
        ];
        # Keep the VM download focused. Other desktops/apps remain installable
        # online; this does not evaluate or build the whole application cache.
        respin.cacheApplications = false;
        respin.referenceDesktops = [
          "plasma"
          "tatami"
        ];
        specialisation = lib.mkForce { };
        isoImage.prependToMenuLabel = lib.mkForce "Plasma (UTM ARM) | ";
        isoImage.makeUsbBootable = lib.mkForce false;
        # Upstream's GRUB menu otherwise offers an x86 test that the ARM EFI
        # image does not contain.
        boot.loader.grub.memtest86.enable = lib.mkForce false;
      })
    ];
  };
  catalog = import ./applications.nix { inherit inputs system; };
in
{
  inherit installer;
  packages = {
    default = installer.config.system.build.isoImage;
    iso = installer.config.system.build.isoImage;
    utm-iso = installer.config.system.build.isoImage;
    inherit genkan;
    vm-fixture = import ./fixture.nix { inherit pkgs; };
    inherit (inputs.tatami.packages.${system})
      tatami
      displays
      network
      activity
      wallpapers
      ;
    inherit (inputs.yukimi.packages.${system}) yukimi;
    installed-lock = pkgs.writeText "sekka-installed-flake.lock" (builtins.toJSON installedLock);
  };
  checks = {
    applications = catalog.check;
    native-installer = genkan;
    yukimi = inputs.yukimi.packages.${system}.yukimi;
    tatami = inputs.tatami.packages.${system}.tatami;
    # Evaluates the complete image as well as the architecture of the exact
    # installed reference system; no x86 CPU firmware or driver closure.
    utm-profile =
      let
        c = installer.config;
      in
      assert installer.pkgs.stdenv.hostPlatform.system == system;
      assert !c.hardware.cpu.intel.updateMicrocode;
      assert !c.hardware.cpu.amd.updateMicrocode;
      assert !(builtins.elem "nvidia" c.services.xserver.videoDrivers);
      assert c.services.qemuGuest.enable && c.services.spice-vdagentd.enable;
      assert c.environment.sessionVariables.GSK_RENDERER == "gl";
      assert builtins.elem "video=Unknown-1:d" c.boot.kernelParams;
      assert builtins.elem "graphical-session.target" c.systemd.user.services.spice-vdagent.wantedBy;
      assert !c.respin.cacheApplications;
      pkgs.writeText "sekka-utm-profile.json" (
        builtins.toJSON {
          inherit system;
          image = builtins.unsafeDiscardStringContext c.system.build.isoImage.drvPath;
          applications = map (a: a.id) catalog.catalog;
        }
      );
  };
}
