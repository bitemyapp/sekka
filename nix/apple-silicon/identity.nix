# This installation's own root and EFI system partition (ESP), for systems
# installed from Sekka's Mac image (calamares.appleSilicon.identity).
#
# Every image is the same bytes, so its root filesystem has the same label and
# filesystem UUID on every Mac (image.nix; none of them a developer image's):
# two product installations on one computer, or an external disk with a copy,
# can't be told apart by them. Only the GPT partition UUIDs the installer
# creates are this installation's own. The installer (installer/genkan/macos,
# handoff.rs) writes them:
# - into the first generation's boot entry, in place of the image's
#   placeholders: sekka.root=PARTUUID=… sekka.esp=PARTUUID=…;
# - into sekka/install.json on the ESP (sekka.mac.handoff/1).
#
# In the initrd, a udev rule names the partitions the command line names
# /dev/disk/by-sekka/{root,esp} (identity/role.sh), and sekka-identity.service
# (identity/check.sh) stops the boot with a clear message when they are
# missing, ambiguous, still placeholders or, on a Mac, not the ESP m1n1 was
# started from. The first generation mounts /dev/disk/by-sekka/*.
#
# At the first start, sekka-install-identity.service checks the handoff
# against the command line and the mounted partitions and writes
# /etc/nixos/identity.nix, which sets `root` and `esp` below: every later
# generation mounts /dev/disk/by-partuuid/* and carries the same arguments.
# After NixOS's boot loader installation, the placeholders in entries it
# rewrote from the first generation are filled in again.
{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.calamares.appleSilicon;
  id = cfg.identity;
  configured = id.root != null;
  placeholder = {
    root = "@SEKKA_ROOT_PARTUUID@";
    esp = "@SEKKA_ESP_PARTUUID@";
  };
  uuid = name: if configured then id.${name} else placeholder.${name};
  device =
    name: if configured then "/dev/disk/by-partuuid/${id.${name}}" else "/dev/disk/by-sekka/${name}";
  python = pkgs.python3.interpreter;

  role = pkgs.writeShellScript "sekka-partition-role" (builtins.readFile ./identity/role.sh);
  rules = pkgs.writeTextDir "lib/udev/rules.d/61-sekka-identity.rules" ''
    # Sekka's per-install identity (nix/apple-silicon/identity.nix): the
    # partitions named by sekka.root=PARTUUID=… and sekka.esp=PARTUUID=… on
    # the kernel command line, as /dev/disk/by-sekka/root and /esp.
    ACTION=="remove", GOTO="sekka_identity_end"
    SUBSYSTEM!="block", GOTO="sekka_identity_end"
    ENV{ID_PART_ENTRY_UUID}!="?*", GOTO="sekka_identity_end"
    IMPORT{program}="${role} $env{ID_PART_ENTRY_UUID}"
    ENV{SEKKA_PARTITION}=="root", SYMLINK+="disk/by-sekka/root"
    ENV{SEKKA_PARTITION}=="esp", SYMLINK+="disk/by-sekka/esp"
    LABEL="sekka_identity_end"
  '';
  installIdentity = ./identity/install_identity.py;
  esp = config.boot.loader.efi.efiSysMountPoint;
  uuidType = lib.types.strMatching "[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}";
in
{
  options.calamares.appleSilicon.identity = {
    handoff = lib.mkEnableOption ''
      this installation's own root and EFI system partition from Sekka's Mac
      installer: the GPT partition UUIDs on the kernel command line
      (sekka.root=, sekka.esp=), then the ones identity.nix records. Developer
      images, installed by the unmodified Asahi installer, have no handoff'';
    root = lib.mkOption {
      type = lib.types.nullOr uuidType;
      default = null;
      description = "The root partition's GPT partition UUID (identity.nix, written at the first start).";
    };
    esp = lib.mkOption {
      type = lib.types.nullOr uuidType;
      default = null;
      description = "The EFI system partition's GPT partition UUID (identity.nix, written at the first start).";
    };
    timeout = lib.mkOption {
      type = lib.types.ints.positive;
      default = 20;
      description = "Seconds the initrd waits for the named partitions before it stops with a message.";
    };
  };

  config = lib.mkIf (cfg.enable && id.handoff) (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = (id.root == null) == (id.esp == null);
            message = "calamares.appleSilicon.identity: set both root and esp (identity.nix), or neither.";
          }
          {
            assertion = !configured || id.root != id.esp;
            message = "calamares.appleSilicon.identity: root and esp name the same partition.";
          }
        ];

        fileSystems."/" = {
          device = device "root";
          fsType = "ext4";
          options = [ "noatime" ];
          # The installer grows the partition (expand); this grows ext4.
          autoResize = true;
        };
        fileSystems."/boot" = {
          device = device "esp";
          fsType = "vfat";
          options = [
            "fmask=0077"
            "dmask=0077"
          ];
        };
        boot.kernelParams = [
          "sekka.root=PARTUUID=${uuid "root"}"
          "sekka.esp=PARTUUID=${uuid "esp"}"
        ];

        services.udev.packages = [ rules ];
        boot.initrd.services.udev.packages = [ rules ];
        boot.initrd.systemd.storePaths = [ role ];
        boot.initrd.systemd.services.sekka-identity = {
          description = "Find this installation's root and EFI system partition";
          requiredBy = [ "sysroot.mount" ];
          wantedBy = [ "initrd.target" ];
          before = [
            "sysroot.mount"
            "systemd-fsck-root.service"
            "initrd-root-fs.target"
          ];
          after = [
            "systemd-udevd.service"
            "systemd-udev-trigger.service"
            "systemd-modules-load.service"
          ];
          unitConfig.DefaultDependencies = false;
          environment.SEKKA_TIMEOUT = toString id.timeout;
          serviceConfig = {
            Type = "oneshot";
            RemainAfterExit = true;
            StandardOutput = "journal+console";
            StandardError = "journal+console";
          };
          script = builtins.readFile ./identity/check.sh;
        };

        # NixOS rewrites every generation's entry from its kernel parameters
        # at each boot loader installation, the first generation's (with the
        # placeholders the installer filled) among them.
        boot.loader.systemd-boot.extraInstallCommands = ''
          ${python} ${installIdentity} fill-entries --esp ${esp} ${
            lib.optionalString configured "--root ${id.root} --esp-partuuid ${id.esp}"
          }
        '';
      }

      (lib.mkIf (!configured) {
        # Once: identity.nix, so every later generation names its partitions.
        systemd.services.sekka-install-identity = {
          description = "Record this installation's own partitions in /etc/nixos/identity.nix";
          wantedBy = [ "multi-user.target" ];
          before = [
            "display-manager.service"
            "multi-user.target"
          ];
          after = [ "local-fs.target" ];
          unitConfig.RequiresMountsFor = [
            "/etc/nixos"
            esp
          ];
          path = [ pkgs.util-linux ];
          serviceConfig = {
            Type = "oneshot";
            RemainAfterExit = true;
            ExecStart = "${python} ${installIdentity} record --esp ${esp} --nixos /etc/nixos";
          };
        };
      })
    ]
  );
}
