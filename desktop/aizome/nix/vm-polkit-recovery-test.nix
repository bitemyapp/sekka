# Real polkit/PAM and Yukimi, switching between prebuilt system profiles.
# The fixture replaces only Nix evaluation/building; authorization, the UI,
# job runner, activation and polkit restarts are the production paths.
{
  pkgs,
  self,
  package ? self.packages.${pkgs.stdenv.hostPlatform.system}.aizome,
}:
let
  first = pkgs.writeShellScriptBin "qa-first-package" "exit 0";
  second = pkgs.writeShellScriptBin "qa-second-package" "exit 0";
  facts =
    value:
    builtins.toJSON {
      choices = [
        {
          setting = "qa.install";
          title = "Test packages";
          many = false;
          inherit value;
          options =
            map
              (id: {
                inherit id;
                title = id;
              })
              [
                "none"
                "first"
                "second"
              ];
        }
      ];
    };
  helper = pkgs.writeShellScriptBin "yukimi-helper" ''
    set -eu
    test -f /etc/aizome-polkit-test-vm
    test "$(${pkgs.coreutils}/bin/id -u)" = 0
    case "$*" in
      'change --choose qa.install=first --mode auto') target=first ;;
      'change --choose qa.install=second --mode auto') target=second ;;
      *) echo 'unexpected fixture operation' >&2; exit 1 ;;
    esac
    echo "$target" >> /var/lib/qa-helper-starts
    ${pkgs.systemd}/bin/systemd-run --quiet --wait --pipe \
      /run/booted-system/specialisation/$target/bin/switch-to-configuration test >&2
    echo '{"ok":true}'
  '';
  policy = pkgs.writeTextDir "share/polkit-1/actions/io.github.bitemyapp.Yukimi.policy" ''
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE policyconfig PUBLIC "-//freedesktop//DTD PolicyKit Policy Configuration 1.0//EN" "http://www.freedesktop.org/standards/PolicyKit/1/policyconfig.dtd">
    <policyconfig>
      <action id="io.github.bitemyapp.Yukimi.change-system">
        <description>Install test packages</description>
        <message>Authenticate to install test packages</message>
        <defaults><allow_any>auth_admin</allow_any><allow_inactive>auth_admin</allow_inactive><allow_active>auth_admin</allow_active></defaults>
        <annotate key="org.freedesktop.policykit.exec.path">${helper}/bin/yukimi-helper</annotate>
      </action>
    </policyconfig>
  '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-polkit-recovery";
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
    security.polkit.enablePkexecWrapper = true;
    environment.systemPackages = [
      helper
      policy
    ];
    environment.etc = {
      "aizome-polkit-test-vm".text = "disposable-authentication-test\n";
      "nixos/configuration.nix".text = "{ ... }: { }\n";
      "yukimi/system.json".text = facts "none";
    };
    specialisation.first.configuration = {
      environment.systemPackages = [ first ];
      environment.etc."yukimi/system.json".text = lib.mkForce (facts "first");
    };
    specialisation.second.configuration = {
      environment.systemPackages = [
        first
        second
      ];
      environment.etc."yukimi/system.json".text = lib.mkForce (facts "second");
    };
    systemd.user.settings.Manager.DefaultEnvironment = "AIZOME_KIT_INSPECT=/run/user/1000/auth-inspect";
    systemd.user.tmpfiles.rules = [ "d %t/auth-inspect 0700 - - -" ];
    virtualisation = {
      memorySize = 4096;
      cores = 4;
      resolution = {
        x = 1280;
        y = 800;
      };
      qemu.options = [
        "-vga none"
        "-device virtio-gpu-pci,xres=1280,yres=800"
      ];
    };
  };
  testScript = builtins.readFile ./vm-polkit-recovery-test.py;
}
