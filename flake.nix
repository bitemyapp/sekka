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

  # Sekka's own packages, and numtide's for the AI applications, also
  # configured on Sekka systems by installer/genkan/rust/system/binary-cache.nix
  # (checks.binary-cache).
  nixConfig = {
    extra-substituters = [
      "https://cache.sekka.computer"
      "https://cache.numtide.com?priority=55"
    ];
    extra-trusted-public-keys = [
      "cache.sekka.computer-1:JJsOkmRt3iCwjvJrol8x2a7euU+kMV7iHaDSFzofA+Y="
      "niks3.numtide.com-1:DTx8wZduET09hRmMtKdQDxNNthLQETkc/yaX7M4qK0g="
    ];
  };

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
      hyprlandOverlay = nixpkgs.lib.composeManyExtensions (import ./nix/overlays);
      hyprlandPkgs = pkgs.extend hyprlandOverlay;
      tools = import ./nix/tools.nix { inherit pkgs; };
      pushCache = import ./nix/push-cache.nix { inherit pkgs; };
      publishNightly = import ./nix/publish-nightly.nix { inherit pkgs; };
      kakehashi = import ./nix/kakehashi.nix { inherit pkgs; };
      site = import ./nix/site.nix {
        inherit pkgs;
        revision = self.shortRev or self.dirtyShortRev or "unknown";
        date =
          let
            d = self.lastModifiedDate or "19700101000000";
          in
          "${builtins.substring 0 4 d}-${builtins.substring 4 2 d}-${builtins.substring 6 2 d}";
      };
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
      overlays.default = hyprlandOverlay;
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
      devShells.${system} = {
        kakehashi = kakehashi.shell;
        default = pkgs.mkShell {
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
      };
      nixosConfigurations.installer = installer;
      apps.${system} = {
        update = {
          type = "app";
          program = "${inputs.yukimi.packages.${system}.yukimi}/bin/sekka-update";
          meta.description = "Update an installed Sekka system, migrating predecessor installations automatically";
        };
        push-cache = {
          type = "app";
          program = "${pushCache}/bin/sekka-push-cache";
          inherit (pushCache) meta;
        };
        publish-nightly = {
          type = "app";
          program = "${publishNightly}/bin/sekka-publish-nightly";
          inherit (publishNightly) meta;
        };
      };
      packages.${system} = {
        default = self.packages.${system}.iso;
        iso = installer.config.system.build.isoImage;
        inherit tools calamares;
        chatgpt = (import ./nix/ai-packages.nix inputs.ai-apps.packages.${system}).chatgpt;
        inherit (hyprlandPkgs) aquamarine hyprland;
        genkan = calamares;
        vm-fixture = fixture;
        # The catalog installs this same package (nix/components.nix).
        inherit (inputs.kakehashi.packages.${system}) kakehashi;
        # The release workflow's sandboxed runner (nix/runner.nix).
        runner = import ./nix/runner.nix {
          inherit pkgs;
          inherit (pkgs) lib;
        };
        screenshots = import ./nix/screenshots.nix {
          inherit pkgs;
          genkan = calamares;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        # A development check of Yukimi's Discover; not run with the others.
        yukimi-discover = import ./nix/yukimi-discover.nix {
          inherit pkgs nixpkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
          applicationsModule = inputs.calamares.nixosModules.applications;
        };
        inherit (inputs.tatami.packages.${system})
          tatami
          displays
          network
          activity
          wallpapers
          ;
        inherit (inputs.yukimi.packages.${system}) yukimi;
        installed-lock = pkgs.writeText "sekka-installed-flake.lock" (builtins.toJSON installedLock);
        # An installed system for every desktop the media offers, and one with
        # every free application in the catalog, with unfree packages refused.
        # push-cache publishes their closures, so whatever Sekka's modules
        # change (Hyprland and all that uses its patched Aquamarine) and
        # whatever a catalog application needs downloads rather than compiles.
        installed-systems =
          let
            reference =
              desktops: applications:
              import ./nix/reference-systems.nix {
                inherit
                  inputs
                  applicationCatalog
                  desktops
                  applications
                  ;
                free = true;
              };
            desktops = reference installer.config.respin.referenceDesktops [ "firefox" ];
            applications = reference [ "plasma" ] (
              map (app: app.id) (builtins.filter (app: !(app.unfree or false)) applicationCatalog.catalog)
            );
          in
          pkgs.linkFarm "sekka-installed-systems" (
            map (desktop: {
              name = desktop;
              path = desktops.systems.${desktop}.config.system.build.toplevel;
            }) desktops.desktops
            ++ [
              {
                name = "applications";
                path = applications.systems.plasma.config.system.build.toplevel;
              }
            ]
          );
        application-cache = applicationCatalog.cache;
        # sekka.computer, ready to upload; nothing publishes it.
        inherit site;
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
        chatgpt-plugins = import ./nix/chatgpt/test.nix {
          inherit pkgs;
          chatgpt = self.packages.${system}.chatgpt;
        };
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
        # The live media and installed systems use the caches this flake
        # advertises, with the same keys.
        binary-cache =
          let
            advertised = (import ./flake.nix).nixConfig;
            installed =
              (import ./nix/reference-systems.nix {
                inherit inputs applicationCatalog;
                desktops = [ "tatami" ];
              }).systems.tatami.config;
            uses =
              c:
              builtins.all (name: builtins.all (v: builtins.elem v c.nix.settings.${name}) advertised.${name}) [
                "extra-substituters"
                "extra-trusted-public-keys"
              ];
          in
          assert uses installer.config;
          assert uses installed;
          pkgs.writeText "binary-cache.json" (builtins.toJSON advertised);
        # Tatami's login entry starts uwsm and tatami from the system profile.
        # A store path in it would change display-manager.service with every
        # Tatami build, and Yukimi would stage each such update for the next
        # boot instead of switching to it.
        tatami-session-entry =
          let
            inherit (pkgs) lib;
            installed =
              (import ./nix/reference-systems.nix {
                inherit inputs applicationCatalog;
                desktops = [ "tatami" ];
              }).systems.tatami.config;
            entry = lib.findFirst (
              p: p.name == "tatami-session"
            ) (throw "Tatami offers no login session") installed.services.displayManager.sessionPackages;
            exec = lib.findFirst (lib.hasPrefix "Exec=") "" (lib.splitString "\n" entry.text);
            profile = "/run/current-system/sw/bin/";
            programs = map (lib.removePrefix profile) (
              lib.filter (lib.hasPrefix profile) (lib.splitString " " (lib.removePrefix "Exec=" exec))
            );
            installs =
              name: lib.any (p: (p.meta.mainProgram or null) == name) installed.environment.systemPackages;
          in
          assert !(lib.hasInfix builtins.storeDir entry.text);
          assert
            programs == [
              "uwsm"
              "tatami"
            ];
          assert lib.all installs programs;
          pkgs.writeText "tatami.desktop" entry.text;
        tatami = self.packages.${system}.tatami;
        tuning = import ./nix/tuning-test.nix { inherit pkgs; };
        fish = import ./nix/fish-vm-test.nix {
          inherit pkgs;
          applicationsModule = inputs.calamares.nixosModules.applications;
          inherit (applicationCatalog.specialArgs) applicationPkgs;
        };
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
        network = self.packages.${system}.network;
        activity = self.packages.${system}.activity;
        kakehashi = self.packages.${system}.kakehashi;
        kakehashi-vm = import ./nix/kakehashi-vm-test.nix {
          inherit pkgs;
          kakehashi = self.packages.${system}.kakehashi;
        };
        network-vm = import ./nix/network-vm-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        activity-vm = import ./nix/activity-vm-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        display-recovery-vm = import ./nix/display-recovery-vm-test.nix {
          inherit pkgs;
          inherit (hyprlandPkgs) hyprland;
          inherit (self.packages.${system}) tatami displays;
        };
        displays-cursor-vm = import ./nix/displays-cursor-vm-test.nix {
          inherit pkgs;
          inherit (hyprlandPkgs) hyprland;
          inherit (self.packages.${system}) tatami displays;
        };
        screenshare-sessions = import ./nix/screenshare-sessions-vm-test.nix {
          inherit pkgs;
          inherit (hyprlandPkgs) hyprland;
          unpatchedHyprland = pkgs.hyprland;
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
        # Tatami's patched Elephant and launcher configuration over the
        # first-party desktop entries; stock Elephant is the negative control.
        launcher-search =
          pkgs.runCommand "tatami-launcher-search-tests" { nativeBuildInputs = [ pkgs.python3 ]; }
            ''
              python3 ${./tests/test_launcher_search.py} \
                ${inputs.tatami.packages.${system}.elephant} ${pkgs.elephant} \
                ${./desktop/tatami/config/elephant} \
                ${./apps/yukimi/yukimi/data/io.github.bitemyapp.Yukimi.desktop} \
                ${./apps/kakehashi/linux/computer.sekka.Kakehashi.desktop} \
                ${./desktop/tatami/activity/data/org.tatami.Activity.desktop} \
                ${./desktop/tatami/network/data/org.tatami.Network.desktop} \
                ${./desktop/tatami/displays/data/org.tatami.Displays.desktop} -v
              touch $out
            '';
        tatami-weather =
          pkgs.runCommand "tatami-weather-tests"
            {
              nativeBuildInputs = [
                pkgs.python3
                pkgs.bubblewrap
              ];
            }
            ''
              python3 ${./tests/test_tatami_weather.py} ${self.packages.${system}.tatami}/bin/tatami -v
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
        site = site;
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
