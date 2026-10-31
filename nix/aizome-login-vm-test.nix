# Logging in to Aizome as a fresh install does, in a disposable VM: a Sekka
# system with Aizome its only desktop, so its login screen is Aizome's own
# (aizome-greeter on greetd; GDM comes only with GNOME), and no automatic
# login. The password typed at the login screen starts Aizome's session and
# unlocks the login keyring, which Chromium and Electron apps keep their
# sign-ins in (`--password-store=gnome-libsecret`); a locked keyring would
# ask for the password again, or sign them out.
#
#     nix build .#checks.x86_64-linux.aizome-login -L
{
  pkgs,
  desktopsModule,
}:
let
  inputProbe = pkgs.runCommand "terminal-input-probe" { nativeBuildInputs = [ pkgs.stdenv.cc ]; } ''
    mkdir -p $out/bin
    cc -Wall -Wextra -Werror ${../tests/terminal-input-probe.c} -o $out/bin/codex
    cp $out/bin/codex $out/bin/build-worker
  '';
in
pkgs.testers.runNixOSTest {
  name = "sekka-aizome-login";
  globalTimeout = 1200;
  node.pkgsReadOnly = false;
  nodes.machine =
    { lib, ... }:
    {
      imports = [ desktopsModule ];
      calamares.desktops = [ "aizome" ];
      calamares.defaultDesktop = "aizome";
      calamares.tuning.enable = true;
      # A fresh session must inherit the installer's system keyboard, as
      # the greeter does. US letters alone cannot detect a US fallback.
      services.xserver.xkb.layout = "gb";
      # The test driver needs console logging, which tuning reduces.
      boot.consoleLogLevel = lib.mkForce 7;
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        description = "Demo";
        password = "demo@#";
        linger = true;
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
    import time
    from typing import Any

    def user(command):
        return machine.succeed(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c "
            + shlex.quote(command))

    def type_uk(text):
        # Physical keys, independent of the test driver's US text mapping.
        plain = {" ": "spc", "\n": "ret", ".": "dot", ",": "comma",
                 "-": "minus", "#": "backslash", "\\": "less"}
        shifted = {"@": "apostrophe", '"': "2", "|": "less", "~": "backslash"}
        def key(code, down):
            event: Any = {"events": [{
                "type": "key", "data": {"down": down, "key": {
                    "type": "qcode", "data": code
                }}
            }]}
            assert machine.qmp_client is not None
            machine.qmp_client.send("input-send-event", event)
            time.sleep(0.01)
        for character in text:
            shift = character in shifted
            if shift:
                key("shift", True)
            code = shifted.get(character, plain.get(character, character))
            key(code, True)
            key(code, False)
            if shift:
                key("shift", False)

    machine.start()
    with subtest("the login screen is Aizome's own, on greetd"):
        machine.wait_for_unit("display-manager.service")
        machine.succeed("systemctl cat display-manager.service | grep -i greetd")
        machine.fail("systemctl is-active gdm.service")
        machine.fail("systemctl is-active plasmalogin.service")
        machine.wait_until_succeeds("pgrep -u greeter -x aizome-greeter", timeout=180)
        machine.wait_until_succeeds("pgrep -u greeter -x aizome", timeout=60)
        # One person: straight to the password.
        machine.sleep(8)
        machine.screenshot("login-screen")

    with subtest("the password starts an Aizome session"):
        type_uk("demo@#\n")
        machine.wait_until_succeeds("ls /run/user/1000/aizome.*.sock", timeout=180)
        machine.wait_until_succeeds("pgrep -u demo -x aizome-shell", timeout=60)
        machine.wait_until_fails("pgrep -u greeter -x aizome-greeter", timeout=60)
        pid = machine.succeed("pgrep -u demo -x aizome-shell | head -1").strip()
        env = machine.succeed(f"tr '\\0' '\\n' < /proc/{pid}/environ")
        assert "XDG_CURRENT_DESKTOP=Aizome" in env, env
        assert "AIZOME_SESSION=1" in env, env
        print(machine.succeed(f"readlink -f /proc/{pid}/exe; sha256sum /proc/{pid}/exe"))
        compositor = machine.succeed("pgrep -u demo -x aizome").strip()
        print(machine.succeed(f"readlink -f /proc/{compositor}/exe; sha256sum /proc/{compositor}/exe"))
        machine.fail("test -e /home/demo/.config/aizome/settings.toml")
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

    with subtest("Foot delivers every TUI keystroke while agent child jobs saturate the CPUs"):
        socket = machine.succeed("ls /run/user/1000/aizome.wayland-*.sock").strip()
        user("AIZOME_SOCKET=" + shlex.quote(socket) + " aizomectl spawn -- foot -- ${inputProbe}/bin/codex")
        machine.wait_until_succeeds("test -f /tmp/sekka-keyboard-input", timeout=60)
        harness = machine.succeed("pgrep -u demo -x codex").strip()
        foot = machine.succeed("pgrep -u demo -x foot").strip()
        machine.wait_until_succeeds(f"test $(cut -d ' ' -f 19 /proc/{foot}/stat) = -4", timeout=30)
        machine.wait_until_succeeds(f"test $(cut -d ' ' -f 19 /proc/{harness}/stat) = -4", timeout=30)
        group = machine.succeed(f"cat /proc/{harness}/cgroup")
        assert "/app.slice/" in group and "/session.slice/" not in group, group
        assert "/app-background.slice/" not in group, group
        workers = machine.succeed(f"pgrep -P {harness}").split()
        assert len(workers) == 8, workers
        for worker in workers:
            machine.wait_until_succeeds(f"grep -q '/app-background.slice/' /proc/{worker}/cgroup")
        # QEMU's emulated keyboard -> Aizome -> actual Foot -> raw TUI input.
        # Include repetitions, spaces and punctuation; compare every byte.
        expected = 'codex input 0123456789 abcdefghijklmnopqrstuvwxyz .,- layout @ # " | ~ \\ ' * 10
        machine.screenshot("foot-ready-under-agent-load")
        # Explicit releases avoid send_chars/sendkey's virtual-time hold
        # overlapping later keys when the loaded guest lags wall time.
        type_uk(expected)
        machine.sleep(3)
        actual = machine.succeed("cat /tmp/sekka-keyboard-input")
        machine.screenshot("foot-input-under-agent-load")
        assert actual == expected, (len(expected), len(actual), expected, actual)
        assert "XKB_DEFAULT_LAYOUT=gb" in env.splitlines(), env
        assert "XKB_DEFAULT_MODEL=pc104" in env.splitlines(), env
        assert "XKB_DEFAULT_VARIANT=" in env.splitlines(), env
        assert "XKB_DEFAULT_OPTIONS=terminate:ctrl_alt_bksp" in env.splitlines(), env

    with subtest("keyboard defaults belong to this session and leave the lingering manager on logout"):
        keys = ("XKB_DEFAULT_LAYOUT", "XKB_DEFAULT_MODEL", "XKB_DEFAULT_VARIANT", "XKB_DEFAULT_OPTIONS")
        snapshot = dict(line.split("=", 1) for line in env.splitlines() if "=" in line)
        managed = snapshot["AIZOME_ENVIRONMENT_KEYS"].split()
        manager = user("systemctl --user show-environment").splitlines()
        for name in keys:
            assert name in managed, managed
            assert name + "=" + snapshot[name] in manager, manager
        print({name: snapshot[name] for name in keys})
        session = machine.succeed("loginctl show-user demo -p Display --value").strip()
        assert session, "demo has no display session"
        machine.succeed("loginctl terminate-session " + shlex.quote(session))
        machine.wait_until_fails("pgrep -u demo -x aizome", timeout=60)
        machine.wait_until_fails("loginctl show-session " + shlex.quote(session), timeout=60)
        assert user("systemctl --user is-active default.target").strip() == "active"
        machine.wait_until_succeeds("pgrep -u greeter -x aizome-greeter", timeout=60)
        manager = user("systemctl --user show-environment").splitlines()
        for name in (*keys, "AIZOME_ENVIRONMENT_KEYS"):
            assert not any(line.startswith(name + "=") for line in manager), manager
        machine.screenshot("keyboard-environment-cleared-at-logout")
  '';
}
