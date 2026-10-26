# Actual UDisks/LUKS on disposable USB and internal disks; real packaged Hako input.
{
  self,
  pkgs,
  package ? self.packages.${pkgs.stdenv.hostPlatform.system}.aizome,
  testCase ? "all",
}:
let
  blankUsb = pkgs.runCommand "aizome-encryption-blank-usb.img" { } ''truncate -s 256M "$out"'';
  fixture = import ./encrypted-volumes-fixture.nix { inherit self pkgs; };
in
pkgs.testers.runNixOSTest {
  name = "aizome-encrypted-volumes";
  enableOCR = true;
  globalTimeout = 420;
  nodes.machine = { lib, ... }: {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    programs.aizome.package = lib.mkForce package;
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      password = "test-only";
      extraGroups = [ "wheel" ];
    };
    services.greetd = {
      enable = true;
      settings.initial_session = {
        user = "alice";
        command = "/run/current-system/sw/bin/aizome-session";
      };
      settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
    };
    environment.systemPackages = [
      fixture
      pkgs.cryptsetup
      pkgs.e2fsprogs
    ];
    environment.etc."aizome-encryption-test-vm".text = "disposable-encryption-test\n";
    environment.etc."aizome-encryption-case".text = testCase;
    environment.etc."aizome-encryption-passphrase" = {
      text = "disposable-volume-pass";
      mode = "0400";
    };
    # Ignore only the whole USB disk: descendants must inherit its hiding policy.
    services.udev.extraRules = ''
      SUBSYSTEM=="block", ENV{DEVTYPE}=="disk", ENV{ID_SERIAL}=="*QA_IGNORED*", ENV{UDISKS_IGNORE}="1"
    '';
    systemd.user.settings.Manager.DefaultEnvironment =
      "AIZOME_KIT_INSPECT=/run/user/1000/encrypted-inspect";
    systemd.user.tmpfiles.rules = [ "d %t/encrypted-inspect 0700 - - -" ];
    virtualisation = {
      memorySize = 4096;
      cores = 4;
      emptyDiskImages = [
        {
          size = 256;
          driveConfig.deviceExtraOpts.serial = "QA_INTERNAL";
        }
      ];
      resolution = {
        x = 1280;
        y = 800;
      };
      qemu.options = [
        "-vga none"
        "-device virtio-gpu-pci,xres=1280,yres=800"
        "-device qemu-xhci,id=qa-xhci"
        "-drive if=none,id=qa-visible,file=${blankUsb},format=raw,snapshot=on"
        "-device usb-storage,bus=qa-xhci.0,drive=qa-visible,serial=QA_VISIBLE"
        "-drive if=none,id=qa-ignored,file=${blankUsb},format=raw,snapshot=on"
        "-device usb-storage,bus=qa-xhci.0,drive=qa-ignored,serial=QA_IGNORED"
      ];
    };
  };
  testScript = builtins.readFile ./vm-encrypted-volumes-test.py;
}
