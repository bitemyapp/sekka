# Sekka for the Asahi installer, starting with the 14-inch MacBook Pro with an
# M3 Pro (J514S). The Asahi installer, run from macOS, shrinks macOS, creates
# the EFI system partition and the root partition, and writes them from an OS
# package: a zip of the partition's files (esp/) and a raw filesystem image
# (root.img).
#
# Two images come from one installed-system configuration:
# - the product image (mac-release-j514s): product/, the configuration that
#   also ships in its /etc/nixos. It has no account and no password, and it
#   finds its partitions by the per-install identity Sekka's Mac installer
#   hands off (identity.nix). The release set adds the manifest
#   (sekka.mac.manifest/1) that a catalog binds.
# - the developer image (mac-installer-j514s): developer/, the same
#   configuration plus developer.nix (the `sekka` account and the Asahi
#   installer's fixed labels), with installer_data.json and install-sekka.sh
#   for the unmodified Asahi installer, which hands nothing off.
#
# Each image's system is evaluated by calling its own /etc/nixos flake.nix with
# the inputs its shipped flake.lock locks (installed-flake.nix), so rebuilding
# /etc/nixos unchanged gives the same system. The root image also holds the
# sources that evaluation reads (seeds), so it works offline.
#
# The installer's m1n1 first stage chainloads m1n1/boot.bin from the new EFI
# system partition; it also copies Apple's firmware for this Mac to vendorfw/
# there (copy_firmware). The ESP is what NixOS's systemd-boot would write for
# the first generation, with the firmware archive as the entry's first initrd;
# NixOS takes it over at the first rebuild. The root image holds the system's
# closure, registered in the Nix database on first boot, and grows to fill its
# partition.
{
  inputs,
  nixpkgs,
  self,
  revision,
}:
let
  system = "aarch64-linux";
  pkgs = nixpkgs.legacyPackages.${system};
  inherit (nixpkgs) lib;

  name = "sekka-j514s-${revision}";
  # Each image's root filesystem label and UUID and the FAT serial the
  # installer gives its EFI system partition (newfs_msdos -I; its
  # /dev/disk/by-uuid name). A developer install mounts by its own
  # (developer.nix, and the original J514S installation's configuration),
  # so the product image shares none of them: its root has another label
  # and UUID, and its ESP no fixed serial (newfs_msdos picks one per
  # installation). Installed beside a developer install, neither system
  # can then mount the other's partitions. The product finds its own by GPT
  # partition UUID (identity.nix).
  identities = lib.importJSON ./identities.json;
  checkedIdentities =
    let
      d = identities.developer;
      p = identities.product;
    in
    assert lib.assertMsg
      (p.root_label != d.root_label && p.root_uuid != d.root_uuid && p.esp_volume_id == null)
      "nix/apple-silicon/identities.json: the product image must share no identity with the developer image's";
    identities;
  # The installer's ESP for UEFI-only installs is 500 MB; NixOS keeps a
  # kernel and initrd per generation there, so this one is larger
  # (installer/genkan/macos, plan.rs ESP_BYTES).
  espSize = 1024 * 1024 * 1024;
  # The stub macOS the Asahi installer creates (plan.rs STUB_BYTES).
  stubBytes = 2500000000 / 1048576 * 1048576;

  installed = import ./installed-flake.nix { inherit lib self; };
  lockFile = pkgs.writeText "flake.lock" (builtins.toJSON installed.lock);
  # The tracked /etc/nixos files must be exactly what the template renders.
  same =
    what: a: b:
    if a == b then true else throw "nix/apple-silicon: ${what} differs from what it must equal";
  checked =
    assert same "product/flake.nix" (builtins.readFile ./product/flake.nix) installed.flake;
    assert same "developer/flake.nix" (builtins.readFile ./developer/flake.nix) installed.flake;
    assert same "developer/configuration.nix" (builtins.readFile ./developer/configuration.nix) (
      builtins.readFile ./product/configuration.nix
    );
    assert same "developer/imports.nix" (builtins.readFile ./developer/imports.nix) (
      builtins.readFile ./product/imports.nix
    );
    true;

  # The sources evaluating /etc/nixos reads, by their path in the lock (from
  # Sekka's own inputs; the installed lock nests them beneath Sekka), and
  # Sekka's own source. This is the set an evaluation in an empty store with
  # no network needed (2026-11-06): the rest of the lock is never fetched by
  # it, as it holds other platforms' Nix daemons, Nix's and its tools' own
  # development inputs, and the catalog's AI, oh-my-pi, Superluminal, Tracy
  # and Uta applications, which the image doesn't select. A selection or
  # input update that needs another source fails the offline evaluation in
  # the VM test (boot-image-in-vm.sh), which names the missing input.
  seedInputs = [
    [ "nixpkgs" ]
    [ "applications" ]
    [ "determinate" ]
    [
      "determinate"
      "nixpkgs"
    ]
    [
      "determinate"
      "determinate-nixd-aarch64-linux"
    ]
    [
      "determinate"
      "nix"
    ]
    [
      "determinate"
      "nix"
      "nixpkgs"
    ]
    [
      "determinate"
      "nix"
      "bdwgc"
    ]
    [ "fh" ]
    [
      "fh"
      "nixpkgs"
    ]
    [
      "fh"
      "crane"
    ]
    [
      "fh"
      "fenix"
    ]
  ];
  seedOf =
    path:
    let
      input = lib.foldl' (acc: n: acc.inputs.${n}) { inherit (self) inputs; } path;
    in
    input.outPath;
  seeds = lib.unique ([ self.outPath ] ++ map seedOf seedInputs);

  python = pkgs.python3.interpreter;

  build =
    {
      variant,
      dir,
      etcFiles,
      title,
    }:
    rec {
      identity = checkedIdentities.${variant};
      nixos =
        assert checked;
        ((import (dir + "/flake.nix")).outputs {
          inherit (inputs)
            nixpkgs
            determinate
            fh
            applications
            ai-apps
            omp
            ;
          sekka = self;
        }).nixosConfigurations.sekka;
      inherit (nixos.config.system.build) toplevel;
      bootBin = nixos.config.system.build.appleSiliconBootBin;
      handoff = nixos.config.calamares.appleSilicon.identity.handoff;

      # /etc/nixos: the flake, its exact lock and the configuration.
      etcNixos = pkgs.runCommand "${name}-${variant}-etc-nixos" { } ''
        mkdir -p $out
        ${lib.concatMapStrings (f: "cp ${dir + "/${f}"} $out/${f}\n") etcFiles}
        cp ${lockFile} $out/flake.lock
        chmod 644 $out/*
      '';

      # What the first rebuilds read beyond the system's own closure: the
      # first generation that names this installation's partitions
      # (identity.nix) changes the initrd, /etc and the system itself, and
      # the owner's choices (setup.nix) more (afterSetup, below), whose build
      # inputs (the initrd's modules, units and splash, the generic builder,
      # the tools that write /etc's files, the packages a system's package
      # list holds but its build of it doesn't keep) aren't in a system's
      # closure. Kept from garbage collection by a root in the image
      # (/nix/var/nix/gcroots/sekka-offline-rebuild), so they build offline.
      # A derivation's inputs, as a file naming them (stdenv's inputDerivation
      # misses structured attributes, which the initrd uses).
      inputsOf =
        drv:
        pkgs.writeText "${drv.name}-inputs.json" (builtins.toJSON (removeAttrs drv.drvAttrs [ "name" ]));

      # configuration.nix as first-boot setup leaves it for `desktop`: its
      # default desktop changed in place, setup's one edit of it (Yukimi's own
      # yukimi_config::edit::set_string, first-boot-setup.md), which
      # yukimi-edits.rs holds to this replacement. Its import of imports.nix
      # is left out: what imports.nix would import on the Mac, identity.nix
      # and setup.nix, comes with it (afterSetup).
      configuredFor =
        desktop:
        let
          shipped = builtins.readFile (dir + "/configuration.nix");
          literal = d: "defaultDesktop = ${builtins.toJSON d};";
          default = literal nixos.config.calamares.defaultDesktop;
          once = text: lib.length (lib.splitString text shipped) == 2;
        in
        assert lib.assertMsg (once default && once "./imports.nix")
          "${variant}/configuration.nix must name its default desktop once, as ${default}, and import ./imports.nix once";
        builtins.toFile "configuration.nix" (
          builtins.replaceStrings [ default "./imports.nix" ] [ (literal desktop) "" ] shipped
        );

      # Systems as first-boot setup leaves them: configuration.nix edited,
      # and identity.nix and setup.nix as install_identity.py and render.rs
      # write them (setup.nix names no default desktop: configuration.nix
      # does), for languages, keyboards and default desktops other than the
      # image's. The owner's account (its shell), host name and choices reach
      # the system's packages, /etc (the login screen's configuration, the
      # keyboard, the host name, Tatami's settings), its units and its checks
      # (the keyboard's validation): the first rebuild after setup rebuilds
      # what differs from the image's own, with these inputs and other
      # strings. Each evaluates the shipped configuration.nix as setup leaves
      # it, so a desktop setup offers whose result doesn't evaluate fails the
      # image, and mac-image-tests (tests.nix, setups).
      afterSetup =
        {
          locale,
          layout,
          desktop,
        }:
        nixos.extendModules {
          modules = [
            { disabledModules = [ (dir + "/configuration.nix") ]; }
            (configuredFor desktop)
            # identity.nix
            {
              calamares.appleSilicon.identity = {
                root = "63d89be1-72f7-4aff-9679-09cafa7e0c2d";
                esp = "910de5ca-7390-4716-9a54-8028916e4d78";
              };
            }
            # setup.nix
            {
              calamares.firstBootSetup.applied = true;
              calamares.installUser = "ada";
              users.users.ada = {
                isNormalUser = true;
                uid = 1000;
                description = "Ada Lovelace";
                extraGroups = [
                  "networkmanager"
                  "wheel"
                ];
                shell = "/run/current-system/sw/bin/bash";
              };
              networking.hostName = "analytical";
              services.xserver.xkb.layout = layout;
              i18n.defaultLocale = locale;
            }
          ];
        };
      # What a system builds that can change with its configuration, by name.
      parts =
        c:
        {
          "system-path" = c.system.path;
        }
        // lib.mapAttrs' (
          n: e: lib.nameValuePair "etc ${n}" (if e.enable then e.source else null)
        ) c.environment.etc
        // lib.mapAttrs' (n: u: lib.nameValuePair "unit ${n}" (u.unit or null)) c.systemd.units
        // lib.mapAttrs' (n: u: lib.nameValuePair "user unit ${n}" (u.unit or null)) c.systemd.user.units
        // lib.mapAttrs' (
          n: u: lib.nameValuePair "initrd unit ${n}" (u.unit or null)
        ) c.boot.initrd.systemd.units
        // lib.listToAttrs (lib.imap0 (i: d: lib.nameValuePair "check ${toString i}" d) c.system.checks)
        // lib.optionalAttrs (c.documentation.man.man-db.enable or false) {
          "man pages" = c.documentation.man.man-db.manualPages;
        };
      # The parts a setup's system builds anew: a derivation where the
      # image's has another or none.
      rebuiltAfterSetup =
        c:
        let
          own = parts nixos.config;
        in
        lib.attrValues (
          lib.filterAttrs (
            n: d: lib.isDerivation d && !(lib.isDerivation (own.${n} or null) && own.${n}.drvPath == d.drvPath)
          ) (parts c)
        );
      # An owner for every desktop setup offers, in turn with these
      # languages and keyboards.
      setups = lib.optionals nixos.config.calamares.firstBootSetup.enable (
        let
          others = [
            {
              locale = "de_DE.UTF-8";
              layout = "de";
            }
            {
              locale = "ja_JP.UTF-8";
              layout = "jp";
            }
          ];
        in
        lib.imap0 (
          i: desktop:
          let
            e = afterSetup (lib.elemAt others (lib.mod i (lib.length others)) // { inherit desktop; });
          in
          # The chosen desktop, defined once: by configuration.nix as setup
          # edits it, and by nothing else.
          assert lib.assertMsg
            (map (d: d.value) e.options.calamares.defaultDesktop.definitionsWithLocations == [ desktop ])
            "${variant}: after setup chose ${desktop}, configuration.nix alone must name it as the default desktop";
          e.config
        ) nixos.config.calamares.desktops
      );
      setupInputs = pkgs.writeText "${name}-${variant}-after-setup-inputs.json" (
        builtins.toJSON (
          map (d: removeAttrs d.drvAttrs [ "name" ]) (lib.concatMap rebuiltAfterSetup setups)
        )
      );
      rebuildInputs = pkgs.linkFarm "${name}-${variant}-rebuild-inputs" (
        [
          {
            name = "stdenv";
            path = pkgs.stdenvNoCC;
          }
          {
            # The initrd's units are linked together with it.
            name = "lndir";
            path = pkgs.lndir;
          }
          {
            name = "initrd";
            path = inputsOf nixos.config.system.build.initialRamdisk;
          }
          {
            name = "etc";
            path = inputsOf nixos.config.system.build.etc;
          }
          {
            name = "system";
            path = inputsOf toplevel;
          }
        ]
        # The console's keymap, made from the keyboard layout setup chooses.
        ++ lib.optional (lib.isDerivation nixos.config.console.keyMap) {
          name = "console-keymap";
          path = inputsOf nixos.config.console.keyMap;
        }
        ++ lib.optional nixos.config.calamares.firstBootSetup.enable {
          name = "after-setup";
          path = setupInputs;
        }
      );

      # The seeds, under a garbage-collection root of their own
      # (/nix/var/nix/gcroots/sekka-offline-sources): no closure the system
      # keeps holds them, so a collection (Yukimi's Storage page, or Nix
      # freeing space for a build) would take them, and the next offline
      # evaluation of /etc/nixos would fail to fetch them again. Two inputs
      # with the same contents are one store path once Nix's lazy trees turn
      # them into store paths, which lib.unique can't see while evaluating,
      # so each is linked once here.
      offlineSources = pkgs.runCommand "${name}-${variant}-offline-sources" { } ''
        mkdir $out
        for source in ${lib.escapeShellArgs seeds}; do
          link=$out/$(basename "$source")
          [ -e "$link" ] || ln -s "$source" "$link"
        done
      '';

      storePaths = [
        toplevel
        rebuildInputs
        offlineSources
      ];
      closureInfo = pkgs.closureInfo { rootPaths = storePaths; };
      rootImage = pkgs.callPackage "${nixpkgs}/nixos/lib/make-ext4-fs.nix" {
        inherit storePaths;
        volumeLabel = identity.root_label;
        uuid = identity.root_uuid;
        populateImageCommands = ''
          mkdir -p ./files/etc/nixos ./files/nix/var/nix/gcroots
          cp ${etcNixos}/* ./files/etc/nixos/
          chmod 755 ./files/etc ./files/etc/nixos
          chmod 644 ./files/etc/nixos/*
          ln -s ${rebuildInputs} ./files/nix/var/nix/gcroots/sekka-offline-rebuild
          ln -s ${offlineSources} ./files/nix/var/nix/gcroots/sekka-offline-sources
        '';
      };

      # What NixOS's systemd-boot writes for generation 1, plus the firmware
      # and the boot.bin record (boot-bin/boot_bin.py). The product entry
      # carries the handoff's placeholders, once each.
      esp =
        pkgs.runCommand "${name}-${variant}-esp"
          {
            nativeBuildInputs = [ pkgs.python3 ];
          }
          ''
            mkdir -p $out/m1n1 $out/EFI/BOOT $out/EFI/systemd $out/EFI/nixos $out/loader/entries $out/sekka
            cp ${bootBin} $out/m1n1/boot.bin
            boot=${nixos.config.systemd.package}/lib/systemd/boot/efi/systemd-bootaa64.efi
            cp $boot $out/EFI/BOOT/BOOTAA64.EFI
            cp $boot $out/EFI/systemd/systemd-bootaa64.efi
            cp ${toplevel}/kernel $out/EFI/nixos/kernel.efi
            cp ${toplevel}/initrd $out/EFI/nixos/initrd.efi
            printf 'timeout 3\ndefault nixos-generation-1.conf\nconsole-mode keep\neditor no\n' \
              > $out/loader/loader.conf
            entry=$out/loader/entries/nixos-generation-1.conf
            {
              echo "title Sekka"
              echo "version Generation 1 (${title} ${revision})"
              echo "linux /EFI/nixos/kernel.efi"
              echo "initrd /vendorfw/firmware.cpio"
              echo "initrd /EFI/nixos/initrd.efi"
              echo "options init=${toplevel}/init $(cat ${toplevel}/kernel-params)"
            } > $entry
            for p in @SEKKA_ROOT_PARTUUID@ @SEKKA_ESP_PARTUUID@; do
              n=$( (grep -o "$p" $entry || true) | wc -l)
              if [ "$n" != ${if handoff then "1" else "0"} ]; then
                echo "$entry has $p $n times" >&2
                exit 1
              fi
            done
            # The record boot_bin.py keeps, for the copy the image installs,
            # which the first start marks as booted.
            PYTHONPATH=${./boot-bin} ${python} -c '
            import sys, boot_bin as b
            data = open(sys.argv[1], "rb").read()
            current = b.entry(data, source=sys.argv[1], system=sys.argv[2], booted=False)
            record = dict(b.empty_record(), current=current, history=[current])
            open(sys.argv[3], "wb").write(b.canonical(record))
            ' ${bootBin} ${toplevel} $out/sekka/boot-bin.json
          '';

      # Before packaging: the registration lists exactly the closure, every
      # path is in the image's bytes with its registered NAR hash, and
      # /etc/nixos holds the shipped files (release/check_image.py).
      imageCheck =
        pkgs.runCommand "${name}-${variant}-image-check"
          {
            nativeBuildInputs = [
              pkgs.e2fsprogs
              pkgs.python3
            ];
          }
          ''
            python3 ${./release/check_image.py} \
              --image ${rootImage} --closure-info ${closureInfo} --etc-nixos ${etcNixos} \
              --label ${identity.root_label} --uuid ${identity.root_uuid} --report $out
          '';

      payload =
        pkgs.runCommand "${name}.zip"
          {
            nativeBuildInputs = [ pkgs.zip ];
            # Packaged only after the image passed its checks.
            checkedImage = imageCheck;
          }
          ''
            size=$(stat -c %s ${rootImage})
            if (( size % 4096 )); then
              echo "root.img is $size bytes, not a multiple of 4 KiB" >&2
              exit 1
            fi
            mkdir work && cd work
            cp -r ${esp} esp
            chmod -R u+w esp
            ln -s ${rootImage} root.img
            # Stored, not compressed: the package is read from this disk.
            zip -0 -r -X -q $out esp root.img
          '';

      installerData =
        pkgs.runCommand "installer_data.json"
          {
            text = builtins.toJSON {
              os_list = [
                {
                  name = "Sekka (14-inch MacBook Pro M3 Pro, ${title} ${revision})";
                  default_os_name = "Sekka";
                  boot_object = "m1n1.bin";
                  next_object = "m1n1/boot.bin";
                  package = "${name}.zip";
                  # The macOS 14.8.3 OS firmware Asahi gives M3 Macs, which the GPU
                  # and display drivers were tested with.
                  supported_fw = [ "14.8.3" ];
                  partitions = [
                    (
                      {
                        name = "EFI";
                        type = "EFI";
                        size = "${toString espSize}B";
                        format = "fat";
                        copy_firmware = true;
                        copy_installer_data = true;
                        source = "esp";
                      }
                      // lib.optionalAttrs (identity.esp_volume_id != null) { volume_id = identity.esp_volume_id; }
                    )
                    {
                      name = "Root";
                      type = "Linux";
                      size = "@ROOT_SIZE@B";
                      expand = true;
                      image = "root.img";
                    }
                  ];
                }
              ];
            };
            passAsFile = [ "text" ];
          }
          ''
            substitute $textPath $out --replace-fail @ROOT_SIZE@ "$(stat -c %s ${rootImage})"
          '';
    };

  product = build {
    variant = "product";
    dir = ./product;
    etcFiles = [
      "flake.nix"
      "configuration.nix"
      "imports.nix"
    ];
    title = "image";
  };

  developer = build {
    variant = "developer";
    dir = ./developer;
    etcFiles = [
      "flake.nix"
      "configuration.nix"
      "imports.nix"
      "developer.nix"
    ];
    title = "developer image";
  };

  # The release set the macOS app consumes: the payload, its installer data,
  # the manifest and the shipped /etc/nixos. It carries no installer script:
  # installed without the handoff, the product image stops in its initrd
  # and says why.
  release =
    pkgs.runCommand "${name}-mac-release"
      {
        nativeBuildInputs = [ pkgs.python3 ];
      }
      ''
        mkdir -p $out/os $out/nixos $out/checks
        ln -s ${product.payload} $out/os/${name}.zip
        cp ${product.installerData} $out/installer_data.json
        cp ${product.etcNixos}/* $out/nixos/
        cp ${product.imageCheck} $out/checks/image.txt
        python3 ${./release/manifest.py} \
          --release ${name} --profile j514s --variant product \
          --revision ${
            self.rev
              or (lib.removeSuffix "-dirty" (self.dirtyRev or "0000000000000000000000000000000000000000"))
          } \
          --clean ${if self ? rev then "true" else "false"} \
          --nar-hash ${self.narHash} --update-ref ${installed.ref} \
          --lock ${lockFile} --toplevel ${product.toplevel} --closure-info ${product.closureInfo} \
          --seeds ${pkgs.writeText "seeds" (lib.concatLines seeds)} \
          --root-image ${product.rootImage} --identities ${./identities.json} \
          --esp ${product.esp} --esp-bytes ${toString espSize} \
          --generations ${toString product.nixos.config.boot.loader.systemd-boot.configurationLimit} \
          --payload $out/os/${name}.zip --installer-data $out/installer_data.json \
          --stub-bytes ${toString stubBytes} \
          --out $out/manifest.json
        (cd $out && sha256sum installer_data.json manifest.json os/${name}.zip nixos/* > SHA256SUMS)
      '';
in
{
  inherit product developer release;
  # The directory to run install-sekka.sh from: installer_data.json, the OS
  # package under os/ (the installer's REPO_BASE layout) and the bootstrap.
  bundle = pkgs.runCommand "${name}-mac-installer" { } ''
    mkdir -p $out/os
    ln -s ${developer.payload} $out/os/${name}.zip
    cp ${developer.installerData} $out/installer_data.json
    install -m755 ${./install-sekka.sh} $out/install-sekka.sh
  '';
}
