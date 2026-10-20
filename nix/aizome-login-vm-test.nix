# Logging in to Aizome as a fresh install does, in a disposable VM: a Sekka
# system with Aizome its only desktop, so its login screen is Plasma Login
# Manager (GDM comes only with GNOME), and no automatic login. The password
# typed at the login screen starts Aizome's session and unlocks the login
# keyring, which Chromium and Electron apps keep their sign-ins in
# (`--password-store=gnome-libsecret`); a locked keyring would ask for the
# password again, or sign them out.
#
#     nix build .#checks.x86_64-linux.aizome-login -L
{
  pkgs,
  desktopsModule,
}:
pkgs.testers.runNixOSTest {
  name = "sekka-aizome-login";
  globalTimeout = 1200;
  node.pkgsReadOnly = false;
  nodes.machine =
    { ... }:
    {
      imports = [ desktopsModule ];
      calamares.desktops = [ "aizome" ];
      calamares.defaultDesktop = "aizome";
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        description = "Demo";
        password = "demo";
      };
      virtualisation = {
        memorySize = 4096;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1920,yres=1200"
        ];
      };
      hardware.graphics.enable = true;
      environment.systemPackages = [ pkgs.libsecret ];
    };
  testScript = ''
    import shlex

    def user(command):
        return machine.succeed(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c "
            + shlex.quote(command))

    machine.start()
    with subtest("the login screen is Plasma Login Manager, not GDM"):
        machine.wait_for_unit("display-manager.service")
        machine.succeed("systemctl cat display-manager.service | grep -i plasmalogin")
        machine.fail("systemctl is-active gdm.service")
        machine.wait_until_succeeds("pgrep -f plasma-login-greeter", timeout=180)
        # The greeter takes a moment to draw and focus the password field.
        machine.sleep(15)
        machine.screenshot("login-screen")

    with subtest("the password starts an Aizome session"):
        machine.send_chars("demo\n")
        machine.wait_until_succeeds("ls /run/user/1000/aizome.*.sock", timeout=180)
        machine.wait_until_succeeds("pgrep -u demo -x aizome-shell", timeout=60)
        pid = machine.succeed("pgrep -u demo -x aizome-shell | head -1").strip()
        env = machine.succeed(f"tr '\\0' '\\n' < /proc/{pid}/environ")
        assert "XDG_CURRENT_DESKTOP=Aizome" in env, env
        assert "AIZOME_SESSION=1" in env, env
        machine.sleep(10)
        machine.screenshot("aizome-session")

    with subtest("the login keyring is unlocked by that password"):
        machine.wait_until_succeeds(
            "runuser -u demo -- env DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
            "busctl --user get-property org.freedesktop.secrets "
            "/org/freedesktop/secrets/collection/login "
            "org.freedesktop.Secret.Collection Locked | grep -x 'b false'",
            timeout=60)
        # A secret stores and reads back with no prompt, as an app's would.
        user("printf hunter2 | secret-tool store --label='Aizome login test' app aizome-login-test")
        assert user("secret-tool lookup app aizome-login-test").strip() == "hunter2"
  '';
}
