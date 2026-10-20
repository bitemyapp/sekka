# Aizome's login screen on a Sekka system with three desktops chosen, as the
# installer writes them: Aizome, Tatami (Hyprland underneath) and Xfce (an
# X11 desktop). The login screen lists exactly those three, each once (not
# Hyprland's own sessions or Xfce's Wayland one), and logs in to each:
# Tatami, then Xfce under an X server started with NixOS's arguments, then
# Aizome, remembering the last one. Driven by keys typed into the VM and
# checked through the greeter's inspection files and what runs.
#
#     nix build .#checks.x86_64-linux.aizome-greeter-desktops -L
{
  pkgs,
  desktopsModule,
}:
pkgs.testers.runNixOSTest {
  name = "sekka-aizome-greeter-desktops";
  globalTimeout = 2400;
  node.pkgsReadOnly = false;
  nodes.machine =
    { config, lib, ... }:
    {
      imports = [ desktopsModule ];
      calamares.desktops = [
        "aizome"
        "tatami"
        "xfce"
      ];
      calamares.defaultDesktop = "aizome";
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        description = "Demo";
        password = "demo";
      };
      # What the greeter shows, as files the test reads.
      services.greetd.settings.default_session.command = lib.mkForce "env AIZOME_KIT_INSPECT=/run/greeter-inspect ${config.programs.aizome.package}/bin/aizome-greeter";
      systemd.tmpfiles.rules = [ "d /run/greeter-inspect 0755 greeter greeter -" ];
      virtualisation = {
        memorySize = 6144;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1920,yres=1200"
        ];
      };
      hardware.graphics.enable = true;
      environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
    };
  testScript = ''
    import json, time

    INSPECT = "/run/greeter-inspect"

    def texts():
        out = machine.succeed(f'for f in {INSPECT}/*.json; do [ -f "$f" ] && cat "$f" && echo; done; true')
        docs = [json.loads(l) for l in out.splitlines() if l.strip()]
        return [e["text"] for d in docs for e in d.get("elements", []) if e.get("text") and e.get("visible", True)]

    def until(what, check, timeout=90):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            r = check()
            if r:
                return r
            time.sleep(0.5)
        raise Exception(f"timed out waiting for {what}; the greeter shows {texts()}")

    def shows(text):
        until(repr(text), lambda: any(t.lower() == text.lower() for t in texts()))

    def greeter_up():
        machine.wait_until_succeeds("pgrep -u greeter -x aizome-greeter", timeout=180)
        until("the greeter's elements", lambda: texts())
        time.sleep(1)

    def docs():
        out = machine.succeed(f'for f in {INSPECT}/*.json; do [ -f "$f" ] && cat "$f" && echo; done; true')
        return [json.loads(l) for l in out.splitlines() if l.strip()]

    def menu_open(title):
        until(f"the {title} menu", lambda: any(
            d.get("kind") == "menu" and any(e.get("label") == title for e in d.get("elements", [])) for d in docs()))

    def choose(session):
        machine.send_key("f2")
        menu_open("Sessions")
        machine.send_chars(session.split()[0].lower())
        machine.send_key("ret")
        shows(f"Session: {session}")

    def log_out():
        machine.succeed("loginctl terminate-user demo")
        machine.wait_until_fails("pgrep -u demo", timeout=90)
        machine.succeed(f"rm -f {INSPECT}/*.json")

    machine.start()
    machine.wait_for_unit("display-manager.service")
    machine.succeed("systemctl cat display-manager.service | grep -i greetd")

    with subtest("the login screen offers the three chosen desktops, each once"):
        greeter_up()
        shows("Session: Aizome")
        # What it reads: one entry per chosen desktop, not Hyprland's own
        # sessions or Xfce's Wayland one.
        dirs = machine.succeed("grep -A3 '^sessions' /etc/aizome/greeter.toml | grep -o '/nix/store/[^\"]*'").split()
        entries = sorted(machine.succeed(f"ls {' '.join(dirs)} 2>/dev/null | grep desktop").split())
        assert entries == ["aizome.desktop", "tatami.desktop", "xfce.desktop"], entries
        machine.send_key("f2")
        menu_open("Sessions")
        machine.screenshot("01-sessions")
        machine.send_key("esc")

    with subtest("Tatami"):
        choose("Tatami")
        machine.send_chars("demo\n")
        machine.wait_until_succeeds("pgrep -u demo -f -i hyprland", timeout=180)
        machine.wait_until_fails("pgrep -u greeter -x aizome-greeter", timeout=60)
        time.sleep(8)
        machine.screenshot("02-tatami")
        log_out()

    with subtest("Xfce, under X"):
        greeter_up()
        shows("Session: Tatami")
        choose("Xfce Session")
        machine.send_chars("demo\n")
        machine.wait_until_succeeds("pgrep -u demo -x xfce4-session", timeout=180)
        machine.wait_until_succeeds("pgrep -f 'X(org)? .*vt'", timeout=60)
        time.sleep(8)
        machine.screenshot("03-xfce")
        log_out()

    with subtest("Aizome, and the last session is remembered"):
        greeter_up()
        shows("Session: Xfce Session")
        choose("Aizome")
        machine.send_chars("demo\n")
        machine.wait_until_succeeds("pgrep -u demo -x aizome-shell", timeout=180)
        time.sleep(8)
        machine.screenshot("04-aizome")
        state = machine.succeed("cat /var/lib/aizome-greeter/state.toml")
        assert 'demo = "aizome"' in state, state
  '';
}
