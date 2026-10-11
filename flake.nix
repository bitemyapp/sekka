# Adapted from DeterminateSystems/nixos-iso (Apache-2.0).
# Official graphical base, native Rust installer, unchanged system input lock.
{
  description = "Sekka: NixOS with Genkan, Tatami and Yukimi";
  inputs.nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1";
  inputs.determinate.url = "https://flakehub.com/f/DeterminateSystems/determinate/*";
  inputs.fh.url = "https://flakehub.com/f/DeterminateSystems/fh/*.tar.gz";
  # Application updates are independent of the already-tested base system.
  inputs.applications.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.ai-apps.url = "github:numtide/llm-agents.nix";
  inputs.omp.url = "github:can1357/oh-my-pi/v18.6.1";

  outputs =
    externalInputs@{
      self,
      nixpkgs,
      determinate,
      fh,
      ...
    }:
    let
      inputs = externalInputs // (import ./nix/components.nix { inherit nixpkgs; });
      installedLock = import ./nix/installed-lock.nix { inherit self; };
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      hyprlandPkgs = pkgs.extend (import ./nix/overlays/aquamarine.nix);
      tools = import ./nix/tools.nix { inherit pkgs; };
      fixture = import ./nix/fixture.nix { inherit pkgs; };
      calamares = import ./nix/calamares.nix {
        inherit pkgs;
        source = inputs.calamares;
      };
      applicationCatalog = import ./nix/applications.nix { inherit inputs; };
      installer = nixpkgs.lib.nixosSystem {
        inherit system;
        specialArgs = {
          inherit inputs installedLock;
          sekkaSource = self.outPath;
        };
        modules = [
          determinate.nixosModules.default
          "${nixpkgs}/nixos/modules/installer/cd-dvd/installation-cd-graphical-base.nix"
          ./modules/installer.nix
        ];
      };
      etc = installer.config.environment.etc;
      liveConfigs = [
        installer.config
      ]
      ++ builtins.map (s: s.configuration) (builtins.attrValues installer.config.specialisation);
      menuLabel =
        c:
        c.isoImage.prependToMenuLabel
        + c.system.nixos.distroName
        + " "
        + c.system.nixos.label
        + c.isoImage.appendToMenuLabel;
    in
    {
      overlays.default = import ./nix/overlays/aquamarine.nix;
      nixosModules = {
        default = inputs.calamares.nixosModules.default;
        desktops = inputs.calamares.nixosModules.desktops;
        applications = inputs.calamares.nixosModules.applications;
        tatami = inputs.tatami.nixosModules.default;
        yukimi = inputs.yukimi.nixosModules.default;
      };
      lib = {
        inherit installedLock;
        components = import ./nix/components.nix { inherit nixpkgs; };
      };
      devShells.${system}.default = pkgs.mkShell {
        inputsFrom = [
          calamares
          inputs.yukimi.packages.${system}.yukimi
        ];
        packages = [
          pkgs.cargo
          pkgs.rustc
          pkgs.clippy
          pkgs.rustfmt
          pkgs.nixfmt
          pkgs.python3
          pkgs.pkg-config
        ];
      };
      nixosConfigurations.installer = installer;
      apps.${system}.update = {
        type = "app";
        program = "${inputs.yukimi.packages.${system}.yukimi}/bin/sekka-update";
        meta.description = "Update an installed Sekka system, migrating predecessor installations automatically";
      };
      packages.${system} = {
        default = self.packages.${system}.iso;
        iso = installer.config.system.build.isoImage;
        inherit tools calamares;
        inherit (hyprlandPkgs) aquamarine hyprland;
        genkan = calamares;
        vm-fixture = fixture;
        screenshots = import ./nix/screenshots.nix {
          inherit pkgs;
          genkan = calamares;
          inherit (inputs.yukimi.packages.${system}) yukimi;
          yukimiModule = inputs.yukimi.nixosModules.default;
        };
        inherit (inputs.tatami.packages.${system}) tatami displays wallpapers;
        inherit (inputs.yukimi.packages.${system}) yukimi;
        installed-lock = pkgs.writeText "sekka-installed-flake.lock" (builtins.toJSON installedLock);
        application-cache = applicationCatalog.cache;
        # Exported separately for isolated Wi-Fi regression tests, not shipped
        # in the public ISO or installed as a live access-point service.
        wifi-test-tools = pkgs.symlinkJoin {
          name = "respin-wifi-test-tools";
          paths = [
            pkgs.hostapd
            pkgs.iw
            pkgs.iproute2
            pkgs.iputils
          ];
        };
      };
      checks.${system} = {
        hyprland-packages = import ./nix/hyprland-packages-test.nix {
          inherit pkgs inputs applicationCatalog;
          inherit (hyprlandPkgs) aquamarine hyprland;
        };
        aquamarine = import ./nix/aquamarine-test.nix {
          inherit pkgs;
          inherit (hyprlandPkgs) aquamarine hyprland;
          unpatchedAquamarine = pkgs.aquamarine;
        };
        native-installer = calamares;
        storage = calamares.storageTest;
        applications = applicationCatalog.check;
        application-cache = applicationCatalog.cache;
        live-profiles =
          let
            normal = installer.config;
            lts = normal.specialisation.lts_kernel.configuration;
            recovery = normal.specialisation.compatibility.configuration;
          in
          assert normal.boot.kernelPackages.kernel.version == pkgs.linuxPackages_latest.kernel.version;
          assert lts.boot.kernelPackages.kernel.version == pkgs.linuxPackages.kernel.version;
          assert recovery.boot.kernelPackages.kernel.version == normal.boot.kernelPackages.kernel.version;
          assert normal.services.displayManager.defaultSession == "plasma";
          assert recovery.services.displayManager.defaultSession == "xfce";
          assert recovery.services.xserver.displayManager.lightdm.enable;
          assert !recovery.services.desktopManager.plasma6.enable;
          assert !recovery.services.displayManager.plasma-login-manager.enable;
          assert recovery.environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE == "1";
          # NVIDIA's driver, never nouveau; the fallback uses neither.
          assert builtins.all
            (
              c:
              builtins.elem "nvidia" c.services.xserver.videoDrivers
              && builtins.elem "nouveau" c.boot.blacklistedKernelModules
              && c.hardware.nvidia.open
              # Video memory survives sleep, as on installed systems.
              && c.hardware.nvidia.powerManagement.enable
            )
            [
              normal
              lts
            ];
          # The Plasma live session never sleeps, blanks or locks on its own.
          assert pkgs.lib.hasInfix "[AC][SuspendAndShutdown]\nAutoSuspendAction=0"
            etc."xdg/powerdevilrc".text;
          assert pkgs.lib.hasInfix
            "[Battery][Display]\nDimDisplayWhenIdle=false\nTurnOffDisplayWhenIdle=false"
            etc."xdg/powerdevilrc".text;
          assert pkgs.lib.hasInfix "Autolock=false" etc."xdg/kscreenlockerrc".text;
          assert !(builtins.elem "nvidia" recovery.services.xserver.videoDrivers);
          assert builtins.elem "nouveau" recovery.boot.blacklistedKernelModules;
          assert
            recovery.environment.etc."calamares-nixos/settings.json".text
            == etc."calamares-nixos/settings.json".text;
          assert builtins.all (
            c:
            builtins.stringLength (menuLabel c) <= 64
            && pkgs.lib.hasPrefix c.boot.kernelPackages.kernel.version (menuLabel c)
            && c.isoImage.configurationName == null
            && c.services.libinput.enable
            && c.networking.networkmanager.enable
            && c.networking.networkmanager.wifi.backend == "wpa_supplicant"
            && c.networking.wireless.enable
            && c.networking.wireless.dbusControlled
            && !c.networking.wireless.autoDetectInterfaces
            && c.boot.supportedFilesystems.ext4
            && c.boot.supportedFilesystems.btrfs
            && c.boot.supportedFilesystems.xfs
            && c.boot.supportedFilesystems.vfat
            && c.hardware.enableRedistributableFirmware
            && c.hardware.cpu.intel.updateMicrocode
            && c.hardware.cpu.amd.updateMicrocode
            && c.nixpkgs.config.allowUnfree
            && !c.boot.plymouth.enable
            && !(c.environment.etc ? "nixos-generate-config.conf")
          ) liveConfigs;
          pkgs.writeText "live-profile-checks.json" (
            builtins.toJSON (
              builtins.map (c: {
                label = menuLabel c;
                kernel = c.boot.kernelPackages.kernel.version;
                session = c.services.displayManager.defaultSession;
                wifiBackend = c.networking.networkmanager.wifi.backend;
                supplicantEnabled = c.networking.wireless.enable;
                redistributableFirmware = c.hardware.enableRedistributableFirmware;
                intelMicrocode = c.hardware.cpu.intel.updateMicrocode;
                amdMicrocode = c.hardware.cpu.amd.updateMicrocode;
                allowUnfree = c.nixpkgs.config.allowUnfree;
              }) liveConfigs
            )
          );
        # An installed system with an NVIDIA GPU uses the driver build on the
        # media, for the latest and LTS kernels alike.
        nvidia-on-media =
          let
            installed =
              kernel:
              (nixpkgs.lib.nixosSystem {
                inherit system;
                inherit (applicationCatalog) specialArgs;
                modules = [
                  determinate.nixosModules.default
                  (import ./installer/genkan/rust/reference.nix {
                    desktops = [ "plasma" ];
                    inherit kernel;
                  })
                  inputs.tatami.nixosModules.default
                  inputs.yukimi.nixosModules.default
                  {
                    calamares.nvidia = {
                      enable = true;
                      prime = {
                        nvidiaBusId = "PCI:1:0:0";
                        intelBusId = "PCI:0:2:0";
                      };
                    };
                  }
                ];
              }).config;
            same =
              installed: live: installed.hardware.nvidia.package.outPath == live.hardware.nvidia.package.outPath;
            latest = installed "latest";
          in
          assert same latest installer.config;
          assert same (installed "lts") installer.config.specialisation.lts_kernel.configuration;
          assert latest.hardware.nvidia.open && latest.hardware.nvidia.prime.offload.enable;
          assert latest.hardware.nvidia.powerManagement.finegrained;
          pkgs.writeText "nvidia-on-media" latest.hardware.nvidia.package.version;
        tatami = self.packages.${system}.tatami;
        keep-awake = import ./nix/keep-awake-test.nix {
          inherit pkgs;
          inherit (self.packages.${system}) tatami;
        };
        usb-wake = pkgs.runCommand "usb-wake-tests" { nativeBuildInputs = [ pkgs.python3 ]; } ''
          mkdir -p installer/genkan/rust/system tests
          cp ${./installer/genkan/rust/system/usb-wake.py} installer/genkan/rust/system/usb-wake.py
          cp ${./tests/test_usb_wake.py} tests/test_usb_wake.py
          python3 -m unittest discover -s tests -p test_usb_wake.py -v
          touch $out
        '';
        tatami-background =
          pkgs.runCommand "tatami-background-tests"
            {
              nativeBuildInputs = [ pkgs.python3 ];
            }
            ''
              python3 ${./tests/test_tatami_background.py} ${self.packages.${system}.tatami}/bin/tatami
              touch $out
            '';
        displays = self.packages.${system}.displays;
        display-recovery-vm = import ./nix/display-recovery-vm-test.nix {
          inherit pkgs;
          inherit (hyprlandPkgs) hyprland;
          inherit (self.packages.${system}) tatami displays;
        };
        display-recovery =
          pkgs.runCommand "tatami-display-recovery-tests"
            {
              nativeBuildInputs = [
                pkgs.python3
                pkgs.bubblewrap
                pkgs.coreutils
              ];
            }
            ''
              python3 ${./tests/test_display_recovery.py} ${self.packages.${system}.tatami}/bin/tatami -v
              touch $out
            '';
        wallpapers =
          pkgs.runCommand "wallpaper-originals-check"
            {
              nativeBuildInputs = [ (pkgs.python3.withPackages (p: [ p.pillow ])) ];
            }
            ''
              python3 ${./tests/check_wallpapers.py} ${./desktop/tatami/wallpapers} ${
                self.packages.${system}.wallpapers
              }
              touch $out
            '';
        yukimi = self.packages.${system}.yukimi;
        update-completion = import ./nix/update-completion-test.nix {
          inherit pkgs;
          inherit (self.packages.${system}) yukimi;
          yukimiModule = inputs.yukimi.nixosModules.default;
        };
        tools = tools;
        template-lock = pkgs.runCommand "template-lock-tests" { } ''
          ${tools}/bin/respin-tools check-config template ${./templates/flake.nix.in} ${
            self.packages.${system}.installed-lock
          }
          ${fixture}/bin/calamares-vm-fixture validate-template ${./templates/flake.nix.in} ${
            self.packages.${system}.installed-lock
          } > /dev/null
          touch $out
        '';
        release-assets = pkgs.runCommand "release-asset-tests" { nativeBuildInputs = [ pkgs.python3 ]; } ''
          mkdir -p scripts tests
          cp ${./scripts/prepare-release.py} scripts/prepare-release.py
          cp ${./tests/test_release.py} tests/test_release.py
          cp ${./flake.lock} flake.lock
          python3 -m unittest discover -s tests -p test_release.py
          touch $out
        '';
        release-workflow =
          pkgs.runCommand "release-workflow-check"
            {
              nativeBuildInputs = [
                pkgs.actionlint
                pkgs.shellcheck
              ];
            }
            ''
              # The sole constant-false job is intentional until publication is enabled.
              actionlint -ignore 'constant expression "false" in condition' -config-file ${./.github/actionlint.yaml} ${./.github/workflows/release.yml}
              touch $out
            '';
        kernel-settings =
          let
            lts =
              pkgs.writeText "lts-settings.json"
                installer.config.specialisation.lts_kernel.configuration.environment.etc."calamares-nixos/settings.json".text;
            latest = pkgs.writeText "latest-settings.json" etc."calamares-nixos/settings.json".text;
          in
          assert !(etc ? "nixos-generate-config.conf");
          assert !(etc ? "determinate-installer/flake.lock");
          assert builtins.all (
            p:
            !(builtins.elem (pkgs.lib.getName p) [
              "calamares-nixos"
              "calamares-nixos-extensions"
              "respin-tools"
            ])
          ) installer.config.environment.systemPackages;
          pkgs.runCommand "native-kernel-settings-tests" { } ''
            ${tools}/bin/respin-tools check-config settings ${lts} ${latest}
            touch $out
          '';
      };
      formatter.${system} = pkgs.nixfmt;
    };
}
