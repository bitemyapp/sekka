# X11 must activate the graphical session and its portals, then stop them
# before the next Wayland login, even while the user manager stays alive.
{ pkgs, self }:
pkgs.testers.runNixOSTest {
  name = "aizome-greeter-x11";
  globalTimeout = 900;
  nodes.machine = { config, lib, ... }: {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    programs.aizome.greeter.enable = true;
    services.displayManager.defaultSession = "xfce";
    services.xserver.enable = true;
    services.xserver.desktopManager.xfce.enable = true;
    users.users.demo = {
      isNormalUser = true;
      uid = 1000;
      password = "demo";
      linger = true;
    };
    services.greetd.settings.default_session.command =
      lib.mkForce "env AIZOME_KIT_INSPECT=/run/greeter-inspect ${config.programs.aizome.package}/bin/aizome-greeter";
    systemd.tmpfiles.rules = [ "d /run/greeter-inspect 0755 greeter greeter -" ];
    virtualisation = {
      memorySize = 6144;
      cores = 4;
      qemu.options = [
        "-vga none"
        "-device virtio-gpu-pci,xres=1280,yres=800"
      ];
    };
    environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
  };
  testScript = ''
    import shlex, time

    def user(command):
        return machine.succeed("su demo -c " + shlex.quote(
            "XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus " + command))

    def active(*units):
        for unit in units:
            state = user("systemctl --user show -p ActiveState --value " + unit).strip()
            assert state == "active", (unit, state)

    def greeter():
        machine.wait_until_succeeds("pgrep -u greeter -x aizome-greeter", timeout=120)
        machine.wait_until_succeeds("test -n \"$(find /run/greeter-inspect -name '*.json' -print -quit)\"")
        time.sleep(2)

    def choose(name):
        machine.send_key("f2")
        time.sleep(1)
        machine.send_chars(name)
        machine.send_key("ret")
        time.sleep(1)
        machine.send_chars("demo\n")

    machine.start()
    machine.wait_for_unit("greetd.service")
    greeter()
    choose("xfce")
    machine.wait_until_succeeds("pgrep -u demo -f xfce4-session", timeout=120)
    machine.wait_until_succeeds("pgrep -u demo -f xfce4-panel", timeout=60)
    time.sleep(8)

    with subtest("Xfce starts the graphical target and the GTK file chooser portal"):
        active("graphical-session.target", "nixos-fake-graphical-session.target")
        user("busctl --user get-property org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop org.freedesktop.portal.FileChooser version")
        active("xdg-desktop-portal.service", "xdg-desktop-portal-gtk.service")
        env = user("systemctl --user show-environment")
        assert "XDG_CURRENT_DESKTOP=XFCE" in env, env
        assert "XDG_SESSION_TYPE=x11" in env, env
        assert "DISPLAY=:" in env, env
        machine.screenshot("01-xfce-portal")

    with subtest("the desktop's logout stops the target and portals with a persistent user manager"):
        # Request Xfce's own logout, rather than killing the login/user manager.
        user("busctl --user call org.xfce.SessionManager /org/xfce/SessionManager org.xfce.Session.Manager Logout bb false false")
        machine.wait_until_fails("pgrep -u demo -f xfce4-session", timeout=90)
        greeter()
        user("systemctl --user is-active default.target")
        for unit in ("nixos-fake-graphical-session.target", "graphical-session.target", "xdg-desktop-portal.service", "xdg-desktop-portal-gtk.service"):
            state = user("systemctl --user show -p ActiveState --value " + unit).strip()
            assert state == "inactive", (unit, state)
        env = user("systemctl --user show-environment").splitlines()
        for name in ("DISPLAY", "XAUTHORITY", "XDG_SESSION_ID", "XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "XDG_SESSION_TYPE"):
            assert not any(line.startswith(name + "=") for line in env), env
        machine.screenshot("02-back-at-greeter")

    with subtest("the next Wayland session has its own environment and working portals"):
        choose("aizome")
        machine.wait_until_succeeds("pgrep -u demo -x aizome-shell", timeout=120)
        active("aizome-session.target", "graphical-session.target")
        user("busctl --user get-property org.freedesktop.portal.Desktop /org/freedesktop/portal/desktop org.freedesktop.portal.FileChooser version")
        active("xdg-desktop-portal.service", "xdg-desktop-portal-gtk.service")
        env = user("systemctl --user show-environment")
        assert "XDG_CURRENT_DESKTOP=Aizome" in env, env
        assert "XDG_SESSION_TYPE=wayland" in env, env
        machine.fail("pgrep -u demo -f xfce4-session")
        machine.screenshot("03-aizome-after-xfce")
  '';
}
