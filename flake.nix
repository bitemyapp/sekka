# Adapted from DeterminateSystems/nixos-iso (Apache-2.0).
# Official graphical base, native Rust installer, unchanged system input lock.
{
  description = "Sekka: NixOS with Genkan, Aizome, Tatami and Yukimi";
  inputs.nixpkgs.url = "https://flakehub.com/f/NixOS/nixpkgs/0.1";
  inputs.determinate.url = "https://flakehub.com/f/DeterminateSystems/determinate/*";
  inputs.fh.url = "https://flakehub.com/f/DeterminateSystems/fh/*.tar.gz";
  # Application updates are independent of the already-tested base system.
  inputs.applications.url = "github:NixOS/nixpkgs/nixos-unstable";
  inputs.ai-apps.url = "github:numtide/llm-agents.nix";
  inputs.omp.url = "github:can1357/oh-my-pi/v18.6.1";
  # Catalog applications packaged by other flakes, from forks under bitemyapp:
  # only commits there reach the nightly. Sekka builds them with the
  # applications' Nixpkgs and gives them to the catalog as its own packages
  # (nix/components.nix), so installed systems keep them beneath Sekka
  # (nix/installed-lock.nix) rather than as inputs of their own.
  inputs.superluminal.url = "github:bitemyapp/superluminal-nix-linux";
  inputs.superluminal.inputs.nixpkgs.follows = "applications";
  inputs.tracy.url = "github:bitemyapp/tracy-flake";
  inputs.tracy.inputs.nixpkgs.follows = "applications";
  # Uta, the YouTube Music client, as source: Sekka builds it against
  # Aizome's kit from this checkout (nix/uta.nix), not the Aizome its own
  # lock names. Pinned by revision, so the nightly's input update doesn't
  # move it: a Uta that needs a different Aizome would fail the nightly's
  # checks and hold back every other input. Re-pin as DEVELOPERS.md (Aizome)
  # describes: from a sekka branch of bitemyapp/uta, never its main.
  inputs.uta = {
    url = "github:bitemyapp/uta/a70e8f6fda8f4744ee8bfc27d93657176d9860ff";
    flake = false;
  };

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
      components = import ./nix/components.nix {
        inherit nixpkgs;
        inherit (externalInputs)
          applications
          superluminal
          tracy
          uta
          ;
      };
      inputs = externalInputs // components;
      installedLock = import ./nix/installed-lock.nix { inherit self; };
      system = "x86_64-linux";
      pkgs = nixpkgs.legacyPackages.${system};
      runnerNix = inputs.determinate.inputs.nix.packages.${system}.default;
      hyprlandOverlay = nixpkgs.lib.composeManyExtensions (import ./nix/overlays);
      hyprlandPkgs = pkgs.extend hyprlandOverlay;
      xappOverlay = import ./nix/overlays/xapp.nix;
      xappPkgs = pkgs.extend xappOverlay;
      avahiOverlay = import ./nix/overlays/avahi.nix;
      avahiPkgs = pkgs.extend avahiOverlay;
      pipewireOverlay = import ./nix/overlays/pipewire.nix;
      pipewirePkgs = pkgs.extend pipewireOverlay;
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
      aizomeSite = import ./nix/aizome-site.nix {
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
        fonts = inputs.aizome.packages.${system}.fonts;
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
      # Googlebook construction is temporarily disabled. Its module, builders
      # and fixture remain in source, without configuration/package/check outputs.
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
      overlays.default = nixpkgs.lib.composeManyExtensions [
        hyprlandOverlay
        xappOverlay
        avahiOverlay
        pipewireOverlay
      ];
      nixosModules = {
        default = inputs.calamares.nixosModules.default;
        desktops = inputs.calamares.nixosModules.desktops;
        applications = inputs.calamares.nixosModules.applications;
        tatami = inputs.tatami.nixosModules.default;
        yukimi = inputs.yukimi.nixosModules.default;
        aizome = inputs.aizome.nixosModules.default;
      };
      lib = {
        inherit installedLock;
        inherit components;
      };
      devShells.${system} = {
        kakehashi = kakehashi.shell;
        # Aizome's workspace (desktop/aizome), which its `just` recipes and
        # aizome-dev enter.
        aizome = inputs.aizome.devShells.${system}.default;
        default = pkgs.mkShell {
          inputsFrom = [
            calamares
            inputs.yukimi.packages.${system}.yukimi.helper
          ];
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.clippy
            pkgs.rustfmt
            pkgs.nixfmt
            pkgs.python3
            pkgs.pkg-config
            pkgs.gtk4
            pkgs.libadwaita
            # Tatami's Sound window links libpulse for its level meter.
            pkgs.libpulseaudio
          ];
        };
      };
      nixosConfigurations.installer = installer;
      # Aizome's own VM checks (desktop/aizome/nix), under the names its
      # flake gave them: `nix build .#aizome-checks.session`. Not among
      # `checks`, which CI runs, as they weren't in Aizome's repository.
      legacyPackages.${system}.aizome-checks = inputs.aizome.checks.${system};
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
        iso = import ./nix/verified-iso.nix {
          inherit pkgs;
          iso = installer.config.system.build.isoImage;
        };
        inherit tools calamares;
        chatgpt = (import ./nix/ai-packages.nix inputs.ai-apps.packages.${system}).chatgpt;
        aizome = inputs.aizome.packages.${system}.aizome;
        hotaru = inputs.aizome.packages.${system}.hotaru;
        # Aizome's session fonts in one folder, for $AIZOME_FONT_DIR
        # (desktop/aizome/docs/kit.md).
        aizome-fonts = inputs.aizome.packages.${system}.fonts;
        inherit (hyprlandPkgs) aquamarine hyprland;
        genkan = calamares;
        vm-fixture = fixture;
        # The catalog installs these same packages (nix/components.nix).
        inherit (inputs.catalog.packages.${system}) kakehashi uta;
        # The release workflow's sandboxed runner (nix/runner.nix).
        runner = import ./nix/runner.nix {
          inherit pkgs;
          inherit (pkgs) lib;
          nixPackage = runnerNix;
        };
        screenshots = import ./nix/screenshots.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        # Aizome's README captures (docs/screenshots.md).
        screenshots-aizome = import ./nix/screenshots-aizome.nix {
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
          bluetooth
          sound
          activity
          wallpapers
          ;
        inherit (inputs.yukimi.packages.${system}) yukimi;
        installed-lock = pkgs.writeText "sekka-installed-flake.lock" (builtins.toJSON installedLock);
        # An installed system for every desktop the media offers, one with all
        # of those desktops together, and one with every free application in
        # the catalog, with unfree packages refused.
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
            ++ pkgs.lib.optional (desktops.together != null) {
              name = "together";
              path = desktops.together.config.system.build.toplevel;
            }
            ++ [
              {
                name = "applications";
                path = applications.systems.plasma.config.system.build.toplevel;
              }
            ]
          );
        application-cache = applicationCatalog.cache;
        # sekka.computer and aizome.computer, ready to upload; the sites
        # workflow publishes them from main (DEVELOPERS.md).
        inherit site;
        aizome-site = aizomeSite;
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
        pr-ci = import ./nix/pr-ci-test.nix { inherit pkgs; };
        pipewire-packages = import ./nix/pipewire-packages-test.nix {
          inherit
            pkgs
            inputs
            installer
            applicationCatalog
            ;
          patched = pipewirePkgs.sekka-pipewire-daemon;
        };
        pipewire-x11-disconnect = import ./nix/pipewire-x11-disconnect-test.nix {
          inherit pkgs;
          patched = pipewirePkgs.sekka-pipewire-daemon;
        };
        avahi-packages = import ./nix/avahi-packages-test.nix {
          inherit
            pkgs
            inputs
            installer
            applicationCatalog
            ;
          patched = avahiPkgs.sekka-avahi-daemon;
        };
        avahi-startup = import ./nix/avahi-startup-test.nix {
          inherit pkgs;
          patched = avahiPkgs.sekka-avahi-daemon;
        };
        plasma-login-config = import ./nix/plasma-login-config-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        plasma-login-session = import ./nix/plasma-login-session-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        runner-protocol = import ./nix/runner-protocol-test.nix {
          inherit pkgs;
          runner = self.packages.${system}.runner;
        };
        session-environment = import ./nix/session-environment-vm-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        xapp-packages = import ./nix/xapp-packages-test.nix {
          inherit
            pkgs
            inputs
            applicationCatalog
            installer
            ;
          patched = xappPkgs.xdg-desktop-portal-xapp;
        };
        xapp-settings = import ./nix/xapp-settings-test.nix {
          inherit pkgs;
          patched = xappPkgs.xdg-desktop-portal-xapp;
        };
        hyprlauncher-software = import ./nix/hyprlauncher-software-test.nix { pkgs = hyprlandPkgs; };
        aizome-ibus = import ./nix/aizome-ibus-test.nix { inherit pkgs inputs; };
        aizome-ibus-policy = import ./nix/aizome-ibus-policy-test.nix { inherit pkgs inputs; };
        # Aizome's toolkit's own tests, under a headless sway
        # (desktop/aizome/sukumo).
        sukumo = import ./nix/sukumo-test.nix {
          inherit pkgs;
          fonts = inputs.aizome.packages.${system}.fonts;
        };
        ibus-wayland-serial = import ./nix/ibus-wayland-serial-test.nix { inherit pkgs; };
        mpv-software = import ./nix/mpv-software-test.nix { inherit pkgs inputs; };
        iso-desktop-references = import ./nix/iso-desktop-references-test.nix {
          inherit
            pkgs
            inputs
            applicationCatalog
            fixture
            installer
            ;
        };
        iso-copytoram = import ./nix/iso-copytoram-test.nix { inherit pkgs; };
        genkan-live-media = import ./nix/genkan-live-media-test.nix { inherit pkgs; };
        genkan-preflight = import ./nix/genkan-preflight-test.nix {
          inherit pkgs;
          helper = calamares.helper;
          nixPackage = runnerNix;
        };
        iso-store-integrity = import ./nix/iso-integrity-test.nix { inherit pkgs; };
        iso-copytoram-initrd = import ./nix/iso-copytoram-initrd-test.nix {
          inherit pkgs installer;
        };
        chatgpt-plugins = import ./nix/chatgpt/test.nix {
          inherit pkgs;
          chatgpt = self.packages.${system}.chatgpt;
        };
        hyprland-packages = import ./nix/hyprland-packages-test.nix {
          inherit pkgs inputs applicationCatalog;
          inherit (hyprlandPkgs)
            aquamarine
            hyprland
            hyprtoolkit
            hyprlauncher
            ;
        };
        aquamarine = import ./nix/aquamarine-test.nix {
          inherit pkgs;
          inherit (hyprlandPkgs) aquamarine hyprland;
          unpatchedAquamarine = pkgs.aquamarine;
        };
        native-installer = calamares;
        storage = calamares.storageTest;
        encrypted-storage = import ./installer/genkan/nix/encryption-test.nix { inherit pkgs; };
        encrypted-boot-splash = import ./installer/genkan/nix/encrypted-splash-test.nix { inherit pkgs; };
        applications = applicationCatalog.check;
        uta-installation = import ./nix/uta-installation-test.nix {
          inherit pkgs applicationCatalog installer;
          uta = self.packages.${system}.uta;
        };
        # Uta is third-party code: it runs here, in the build sandbox, not in
        # the workflows' jobs, which go on to hold the cache's secrets.
        uta-rustypipe =
          let
            inherit (self.packages.${system}) uta;
          in
          pkgs.runCommand "uta-rustypipe-check" { } ''
            version=$(${uta}/bin/aizome-uta --version)
            echo "$version"
            grep -qF '(RustyPipe: true)' <<< "$version"
            # The GPL-3.0-only build's notices and directions to its source,
            # and the source push-cache publishes beside it.
            doc=${uta}/share/doc/aizome-uta
            for notice in LICENSE-RustyPipe LICENSE-EJS; do
              if [[ ! -s $doc/$notice ]]; then
                echo "$doc/$notice is missing or empty" >&2
                exit 1
              fi
            done
            grep -qF "github:bitemyapp/uta/${inputs.uta.rev or "(not a Git revision)"}" "$doc/SOURCE"
            test -f ${uta.source}/uta/vendor/rustypipe/LICENSE
            test -f ${uta.source}/aizome/crates/aizome-kit/Cargo.toml
            test -f ${uta.source}/sekka/nix/uta.nix
            test -s ${uta.source}/sekka/nix/patches/uta-mpris-readiness.patch
            echo "$version" > $out
          '';
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
          assert normal.services.displayManager.defaultSession == "aizome";
          assert recovery.services.displayManager.defaultSession == "aizome";
          # Pixman and software GL in the recovery entry alone.
          assert recovery.environment.sessionVariables.AIZOME_RENDERER == "pixman";
          assert recovery.environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE == "1";
          assert pkgs.lib.hasInfix "AIZOME_RENDERER=pixman"
            recovery.environment.etc."xdg/aizome/environment".text;
          assert !(normal.environment.sessionVariables ? AIZOME_RENDERER);
          assert !(lts.environment.sessionVariables ? AIZOME_RENDERER);
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
          # The live session never locks or turns its displays off on its own,
          # and opens on Genkan rather than Aizome's welcome tour.
          assert pkgs.lib.hasInfix "lock_after_minutes = 0\nmonitors_off_after_minutes = 0"
            normal.systemd.tmpfiles.settings."10-installer-desktop"."/home/nixos/.config/aizome/settings.toml".f.argument;
          assert !etc."xdg/autostart/org.aizome.Annai.Welcome.desktop".enable;
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
            # Aizome, logged in automatically, behind Sekka's boot splash.
            && c.programs.aizome.enable
            && c.programs.aizome.greeter.enable
            && c.services.greetd.enable
            && c.services.displayManager.autoLogin.enable
            && c.services.displayManager.autoLogin.user == "nixos"
            && !c.services.desktopManager.plasma6.enable
            && c.boot.plymouth.enable
            && c.boot.plymouth.theme == "sekka"
            # A serial console would otherwise turn the splash into text.
            && builtins.elem "plymouth.ignore-serial-consoles" c.boot.kernelParams
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
                  inputs.aizome.nixosModules.default
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
        cache-coverage =
          pkgs.runCommand "cache-coverage-tests"
            {
              nativeBuildInputs = [
                pkgs.nix
                pkgs.python3
              ];
              TEST_SHELL = "${pkgs.pkgsStatic.busybox}/bin/busybox";
            }
            ''
              mkdir -p scripts tests
              cp ${./scripts/check_cache_coverage.py} scripts/check_cache_coverage.py
              cp ${./tests/test_cache_coverage.py} tests/test_cache_coverage.py
              python3 -m unittest discover -s tests -p test_cache_coverage.py -v
              touch $out
            '';
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
        tatami-recording = import ./nix/tatami-recording-test.nix {
          inherit pkgs;
          inherit (self.packages.${system}) tatami;
          inherit (hyprlandPkgs) hyprland;
        };
        screenshot-notifications = import ./nix/screenshot-notifications-vm-test.nix {
          inherit pkgs;
          inherit (self.packages.${system}) tatami;
          inherit (hyprlandPkgs) hyprland;
        };
        tuning = import ./nix/tuning-test.nix {
          inherit pkgs;
          aizomeSource = inputs.aizome.outPath;
        };
        tuning-policy = import ./nix/scheduling/policy-test.nix { inherit nixpkgs system; };
        boot-splash = import ./nix/boot-splash-test.nix { inherit pkgs; };
        boot-splash-choice = import ./nix/boot-splash-choice-test.nix { inherit pkgs inputs; };
        prebuilt-binaries = import ./nix/prebuilt-binaries-vm-test.nix { inherit pkgs; };
        fish = import ./nix/fish-vm-test.nix {
          inherit pkgs;
          applicationsModule = inputs.calamares.nixosModules.applications;
          inherit (applicationCatalog.specialArgs) applicationPkgs;
        };
        fish-default = import ./nix/fish-default-test.nix { inherit pkgs inputs applicationCatalog; };
        genkan-keyboard = import ./nix/genkan-keyboard-test.nix {
          inherit pkgs;
          genkan = calamares;
          sekkaModule = inputs.calamares.nixosModules.default;
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
              nativeBuildInputs = [ (pkgs.python3.withPackages (p: [ p.pillow ])) ];
            }
            ''
              python3 ${./tests/test_tatami_background.py} ${self.packages.${system}.tatami}/bin/tatami
              touch $out
            '';
        displays = self.packages.${system}.displays;
        network = self.packages.${system}.network;
        bluetooth = self.packages.${system}.bluetooth;
        sound = self.packages.${system}.sound;
        sound-integration = import ./nix/sound-test.nix {
          inherit pkgs;
          inherit (self.packages.${system}) sound;
        };
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
        network-fit-vm = import ./nix/network-fit-vm-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        network-dns-vm = import ./nix/network-dns-vm-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
          network = self.packages.${system}.network;
        };
        wpa-supplicant-scan = import ./nix/wpa-supplicant-scan-test.nix {
          inherit pkgs;
          # The live session, in each of its boot entries, and an installed
          # system.
          systems = liveConfigs ++ [
            (import ./nix/reference-systems.nix {
              inherit inputs applicationCatalog;
              desktops = [ "plasma" ];
            }).systems.plasma.config
          ];
        };
        activity-vm = import ./nix/activity-vm-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        aizome-login = import ./nix/aizome-login-vm-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        aizome-greeter-desktops = import ./nix/aizome-greeter-desktops-vm-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        display-recovery-vm = import ./nix/display-recovery-vm-test.nix {
          inherit pkgs;
          inherit (hyprlandPkgs) hyprland;
          inherit (self.packages.${system}) tatami displays;
        };
        displays-fit-vm = import ./nix/displays-fit-vm-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        window-pop-vm = import ./nix/window-pop-vm-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        background-validation-vm = import ./nix/background-validation-vm-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
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
        capture-cursor-vm = import ./nix/capture-cursor-vm-test.nix {
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
                ${./desktop/aizome/apps/yukimi/data/org.aizome.Yukimi.desktop} \
                ${./apps/kakehashi/linux/computer.sekka.Kakehashi.desktop} \
                ${./desktop/tatami/activity/data/org.tatami.Activity.desktop} \
                ${./desktop/tatami/network/data/org.tatami.Network.desktop} \
                ${./desktop/tatami/bluetooth/data/org.tatami.Bluetooth.desktop} \
                ${./desktop/tatami/sound/data/org.tatami.Sound.desktop} \
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
        yukimi-tatami = self.packages.${system}.yukimi-discover;
        yukimi-x11 = import ./nix/yukimi-x11-test.nix {
          inherit pkgs;
          desktopsModule = inputs.calamares.nixosModules.desktops;
        };
        update-completion = import ./nix/update-completion-test.nix {
          inherit pkgs;
          inherit (self.packages.${system}) yukimi;
          yukimiModule = inputs.yukimi.nixosModules.default;
        };
        tools = tools;
        site = site;
        aizome-site = aizomeSite;
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
        publish-nightly =
          pkgs.runCommand "publish-nightly-tests"
            {
              nativeBuildInputs = [
                pkgs.python3
                pkgs.git
              ];
            }
            ''
              mkdir -p scripts tests
              cp ${./scripts/publish-nightly.py} scripts/publish-nightly.py
              cp ${./tests/test_publish_nightly.py} tests/test_publish_nightly.py
              cp ${./flake.lock} flake.lock
              python3 -m unittest discover -s tests -p test_publish_nightly.py -v
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
              actionlint -config-file ${./.github/actionlint.yaml} ${./.github/workflows/nightly.yml}
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
