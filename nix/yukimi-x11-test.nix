# The native interface remains usable from Sekka's Xfce/X11 session.
{ pkgs, desktopsModule }:
pkgs.testers.runNixOSTest {
  name = "yukimi-x11";
  globalTimeout = 600;
  node.pkgsReadOnly = false;
  nodes.machine = { ... }: {
    imports = [ desktopsModule ];
    calamares.desktops = [ "xfce" ];
    calamares.defaultDesktop = "xfce";
    users.users.demo = {
      isNormalUser = true;
      uid = 1000;
      password = "demo";
    };
    services.displayManager.autoLogin = {
      enable = true;
      user = "demo";
    };
    environment.systemPackages = [ pkgs.xdotool ];
    virtualisation = {
      memorySize = 4096;
      cores = 4;
      resolution = {
        x = 1600;
        y = 1000;
      };
    };
    environment.etc."nixos/configuration.nix".text = ''
      { pkgs, ... }: { environment.systemPackages = [ pkgs.file ]; }
    '';
  };
  testScript = ''
    import shlex, time
    machine.start()
    machine.wait_until_succeeds("pgrep -u demo -f '(^|/)[.]?xfce4-session(-wrapped)?($| )'", timeout=180)
    session_pid = machine.succeed("pgrep -u demo -f '(^|/)[.]?xfce4-session(-wrapped)?($| )'").strip().splitlines()[0]
    session_env = dict(line.split("=", 1) for line in machine.succeed(
        f"tr '\\0' '\\n' < /proc/{session_pid}/environ").splitlines() if "=" in line)
    assert session_env["XDG_SESSION_TYPE"] == "x11", session_env
    machine.wait_until_succeeds("pgrep -u demo xfce4-panel")
    def user(command):
        return machine.succeed("runuser -u demo -- env DISPLAY=" + shlex.quote(session_env["DISPLAY"])
            + " XAUTHORITY=" + shlex.quote(session_env["XAUTHORITY"]) + " XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c " + shlex.quote(command))
    # Xfce's Run dialog, as a person launches a command.
    user("xdotool key alt+F2")
    time.sleep(2)
    user("xdotool type --clearmodifiers yukimi")
    time.sleep(1)
    machine.screenshot("xfce-run-dialog")
    user("xdotool key Return")
    try:
        machine.wait_until_succeeds("pgrep -u demo -f '^/nix/store/[^ ]+/libexec/[.]?aizome-yukimi(-wrapped)?($| )'", timeout=30)
    except Exception:
        machine.screenshot("xfce-launch-failed")
        print(machine.succeed("ps -u demo -o pid,args"))
        print(machine.succeed("journalctl -b _UID=1000 --no-pager -n 100"))
        user("yukimi > /tmp/yukimi-launch.log 2>&1 &")
        time.sleep(5)
        print(machine.succeed("cat /tmp/yukimi-launch.log"))
        raise
    machine.wait_until_succeeds("pgrep -u demo -x weston", timeout=30)
    time.sleep(10)
    pid = machine.succeed("pgrep -u demo -f '^/nix/store/[^ ]+/libexec/[.]?aizome-yukimi(-wrapped)?($| )'").strip()
    weston = machine.succeed("pgrep -u demo -x weston").strip()
    machine.screenshot("xfce-yukimi")
    user("xdotool key ctrl+3")
    time.sleep(3)
    machine.screenshot("xfce-discover")
    # Relaunching must keep the original app, and retire the second compositor.
    user("env -u WAYLAND_DISPLAY XDG_CURRENT_DESKTOP=XFCE yukimi --page installed > /tmp/relaunch.log 2>&1 &")
    time.sleep(5)
    assert machine.succeed("pgrep -u demo -f '^/nix/store/[^ ]+/libexec/[.]?aizome-yukimi(-wrapped)?($| )'").strip() == pid
    assert machine.succeed("pgrep -u demo -x weston").strip() == weston
    machine.screenshot("xfce-relaunch")
    user("xdotool key ctrl+q")
    machine.wait_until_succeeds(f"test ! -e /proc/{pid} && test ! -e /proc/{weston}", timeout=30)
    machine.succeed("test -z \"$(find /run/user/1000 -maxdepth 1 -name 'yukimi-x11.*' -print)\"")
    user("env -u WAYLAND_DISPLAY XDG_CURRENT_DESKTOP=XFCE yukimi > /tmp/reopen.log 2>&1 &")
    machine.wait_until_succeeds("pgrep -u demo -x weston")
    time.sleep(5)
    machine.screenshot("xfce-reopened")
    # The outer X11 close button (Alt+F4) must end the native client as well.
    user("xdotool key alt+F4")
    machine.wait_until_succeeds("! pgrep -u demo -x weston")
    machine.wait_until_succeeds("! pgrep -u demo -f '^/nix/store/[^ ]+/libexec/[.]?aizome-yukimi(-wrapped)?($| )'")
    machine.succeed("test -z \"$(find /run/user/1000 -maxdepth 1 -name 'yukimi-x11.*' -print)\"")
    machine.fail("coredumpctl --no-legend list | grep -i yukimi")
  '';
}
