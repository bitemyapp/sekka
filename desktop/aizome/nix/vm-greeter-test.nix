# Aizome's login screen, in a disposable VM, as a person uses it: greetd
# runs aizome-greeter on Aizome's compositor; two people, and three
# sessions to choose from (Aizome, sway for another Wayland window manager,
# and xterm's X11 session). A wrong password says so; the right one starts
# the session and unlocks the keyring; logging out brings the greeter back
# remembering who and what; another person, another session, an X11 one;
# and the power menu. Driven by keys typed into the VM, checked through the
# greeter's inspection files (its elements and their text) and what runs.
#
#     nix build .#checks.x86_64-linux.greeter -L
{ pkgs, self }:
pkgs.testers.runNixOSTest {
  name = "aizome-greeter";

  nodes.machine =
    { pkgs, ... }:
    {
      imports = [ self.nixosModules.default ];
      programs.aizome.enable = true;
      programs.aizome.greeter.enable = true;
      services.displayManager.defaultSession = "aizome";
      # Another Wayland window manager, and an X11 session.
      programs.sway.enable = true;
      services.xserver.enable = true;
      services.xserver.desktopManager.xterm.enable = true;
      # No 3D in the VM: sway draws with pixman, as Aizome does here.
      environment.variables.WLR_RENDERER = "pixman";
      environment.variables.WLR_NO_HARDWARE_CURSORS = "1";

      users.users.alice = {
        isNormalUser = true;
        description = "Alice Kimura";
        password = "alices-password";
        uid = 1000;
      };
      users.users.bob = {
        isNormalUser = true;
        description = "Bob Tanaka";
        password = "bobs-password";
        uid = 1001;
      };

      # What the greeter shows, as files the test reads (the kit writes
      # them where AIZOME_KIT_INSPECT says).
      services.greetd.settings.default_session.command =
        pkgs.lib.mkForce "env AIZOME_KIT_INSPECT=/run/greeter-inspect ${self.packages.${pkgs.stdenv.hostPlatform.system}.aizome}/bin/aizome-greeter";
      systemd.tmpfiles.rules = [ "d /run/greeter-inspect 0755 greeter greeter -" ];

      environment.systemPackages = [ pkgs.libsecret ];
      virtualisation = {
        memorySize = 4096;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1920,yres=1200"
        ];
      };
    };

  testScript = ''
    import json, time

    INSPECT = "/run/greeter-inspect"

    def docs():
        out = machine.succeed(
            f'for f in {INSPECT}/*.json; do [ -f "$f" ] && cat "$f" && echo; done; true'
        )
        return [json.loads(l) for l in out.splitlines() if l.strip()]

    def texts():
        return [e["text"] for d in docs() for e in d.get("elements", []) if e.get("text") and e.get("visible", True)]

    def until(what, check, timeout=60):
        deadline = time.monotonic() + timeout
        last = None
        while time.monotonic() < deadline:
            try:
                last = check()
            except Exception as e:
                last = e
            if last:
                return last
            time.sleep(0.5)
        raise Exception(f"timed out waiting for {what}: {last!r}; the greeter shows {texts()}")

    def shows(text):
        return until(f"{text!r} on the login screen", lambda: any(t.lower() == text.lower() for t in texts()))

    def menu_open(title):
        return until(
            f"the {title} menu",
            lambda: any(d.get("kind") == "menu" and any(e.get("label") == title for e in d.get("elements", [])) for d in docs()),
        )

    def choose(session):
        key("f2")
        menu_open("Sessions")
        machine.send_chars(session.split()[0].lower())
        key("ret")
        shows(f"Session: {session}")

    def greeter_up():
        machine.wait_until_succeeds("pgrep -u greeter -x aizome-greeter", timeout=120)
        machine.wait_until_succeeds("pgrep -u greeter -x aizome", timeout=60)
        # Its surface drawn and its elements written.
        until("the greeter's elements", lambda: any(t for t in texts()))
        time.sleep(1)

    def session_of(user, program):
        machine.wait_until_succeeds(f"pgrep -u {user} -x {program}", timeout=120)
        machine.wait_until_fails("pgrep -u greeter -x aizome-greeter", timeout=60)

    def log_out(user):
        machine.succeed(f"loginctl terminate-user {user}")
        machine.wait_until_fails(f"pgrep -u {user} -x aizome", timeout=60)
        machine.succeed(f"rm -f {INSPECT}/*.json")

    def key(k, n=1):
        for _ in range(n):
            machine.send_key(k)
            time.sleep(0.3)

    machine.start()
    machine.wait_for_unit("greetd.service")

    with subtest("with sway's wlr portal enabled too, the portal is Aizome's patched one"):
        # programs.sway enables NixOS's xdg.portal.wlr: its package gives the
        # unit, and Aizome's ExecStart (one override, not two drop-ins) runs
        # Aizome's portal, with NixOS's configuration outside Aizome.
        unit = machine.succeed("cat /etc/systemd/user/xdg-desktop-portal-wlr.service.d/*.conf")
        assert "xdg-desktop-portal-wlr-aizome" in unit, unit
        wrapper = unit.split("ExecStart=")[-1].strip()
        script = machine.succeed(f"cat {wrapper}")
        assert "--config=" in script and "Aizome" in script, script

    with subtest("the greeter starts on its own compositor and lists both people"):
        greeter_up()
        shows("Alice Kimura")
        shows("Bob Tanaka")
        machine.screenshot("01-people")
        # Its compositor is the greeter's child, without the shell.
        machine.fail("pgrep -u greeter -x aizome-shell")
        ppid = machine.succeed("ps -o ppid= -p $(pgrep -u greeter -x aizome)").strip()
        assert ppid == machine.succeed("pgrep -u greeter -x aizome-greeter").strip(), ppid

    with subtest("the session menu offers every session there is"):
        key("ret")
        shows("Session: Aizome")
        key("f2")
        menu_open("Sessions")
        machine.screenshot("02-sessions")
        key("esc")
        for name in ("Sway", "xterm", "Aizome"):
            choose(name)

    with subtest("a wrong password says so, and asks again"):
        machine.send_chars("not-her-password\n")
        shows("That password didn't work. Try again.")
        machine.screenshot("03-wrong")

    with subtest("the right one starts Aizome and unlocks the keyring"):
        machine.send_chars("alices-password\n")
        session_of("alice", "aizome")
        machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=60)
        pid = machine.succeed("pgrep -u alice -x aizome-shell | head -1").strip()
        env = machine.succeed(f"tr '\\0' '\\n' < /proc/{pid}/environ")
        assert "XDG_CURRENT_DESKTOP=Aizome" in env, env
        machine.wait_until_succeeds(
            "su alice -c 'DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus busctl --user get-property "
            "org.freedesktop.secrets /org/freedesktop/secrets/collection/login "
            "org.freedesktop.Secret.Collection Locked' | grep -x 'b false'",
            timeout=60,
        )
        time.sleep(5)
        machine.screenshot("04-alice-in-aizome")
        state = machine.succeed("cat /var/lib/aizome-greeter/state.toml")
        assert 'last_user = "alice"' in state and 'alice = "aizome"' in state, state

    with subtest("logging out brings the greeter back, at her password"):
        log_out("alice")
        greeter_up()
        shows("Alice Kimura")
        shows("Log in as someone else")
        machine.screenshot("05-back")

    with subtest("someone else, and another session: Bob in sway"):
        key("esc")
        shows("Bob Tanaka")
        key("right")
        key("ret")
        shows("Bob Tanaka")
        choose("Sway")
        machine.send_chars("bobs-password\n")
        session_of("bob", "sway")
        state = machine.succeed("cat /var/lib/aizome-greeter/state.toml")
        assert 'last_user = "bob"' in state and 'bob = "sway"' in state, state
        machine.succeed("loginctl terminate-user bob")
        machine.wait_until_fails("pgrep -u bob -x sway", timeout=60)
        machine.succeed(f"rm -f {INSPECT}/*.json")

    with subtest("Bob's session is remembered, and an X11 one starts under X"):
        greeter_up()
        shows("Session: Sway")
        choose("xterm")
        machine.send_chars("bobs-password\n")
        machine.wait_until_succeeds("pgrep -u bob -f '/bin/xterm'", timeout=120)
        machine.wait_until_succeeds("pgrep -f '/bin/X|Xorg'", timeout=60)
        time.sleep(3)
        machine.screenshot("06-bob-in-x11")
        machine.succeed("loginctl terminate-user bob")
        machine.wait_until_fails("pgrep -u bob -f '/bin/xterm'", timeout=60)
        machine.succeed(f"rm -f {INSPECT}/*.json")

    with subtest("the power menu opens (Sleep, Restart, Shut Down)"):
        greeter_up()
        key("f3")
        menu_open("Power")
        machine.screenshot("07-power")
        key("esc")
  '';
}
