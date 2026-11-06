# Displays set up in Aizome's Settings, then a Tatami session (#319): a
# Sekka system with both desktops, as the installer writes it, and two
# monitors on one virtio GPU, told apart by their EDID names as two real
# monitors are. The second ("Tall") stands in portrait, left of the main
# one, as Settings keeps it in ~/.config/aizome/settings.toml. Aizome shows
# it so; Tatami, which has no display settings of its own yet, must show it
# the same way: rotated, scaled and arranged as in Aizome, through a reload,
# and with Aizome's settings as they are when the displays next change. A
# display then changed in Tatami's Displays window keeps Tatami's own
# settings, and Aizome keeps its own. Unplugged, a display's rule goes.
#
# Logs in through the login screen, as a person does. The second head is
# read from QEMU (screendump), so the pictures are what the monitor shows.
#
#     nix build .#checks.x86_64-linux.tatami-aizome-displays -L
{
  pkgs,
  desktopsModule,
}:
let
  # Two heads, each with its own EDID name: QEMU's vendor RHT, no serial.
  gpu = builtins.toJSON {
    driver = "virtio-gpu-pci";
    id = "gpu";
    max_outputs = 2;
    outputs = [
      {
        name = "Wide";
        xres = 1280;
        yres = 800;
      }
      {
        name = "Tall";
        xres = 1280;
        yres = 800;
      }
    ];
  };
  # What Settings writes after the second display is turned to 90° and
  # dragged left of the first, the first being the main display
  # (apps/settei/src/panes/displays/model.rs, write_entries). QEMU's heads
  # refresh at 75 Hz.
  aizomeSettings = pkgs.writeText "settings.toml" ''
    [[outputs]]
    make = "RHT"
    model = "Wide"
    mode = "1280x800@75"
    scale = 1.0
    position = [640, 0]
    transform = "normal"
    enabled = true
    primary = true

    [[outputs]]
    make = "RHT"
    model = "Tall"
    mode = "1280x800@75"
    scale = 1.25
    position = [0, 0]
    transform = "90"
    enabled = true
  '';
in
pkgs.testers.runNixOSTest {
  name = "sekka-tatami-aizome-displays";
  globalTimeout = 1800;
  node.pkgsReadOnly = false;
  nodes.machine =
    { config, lib, ... }:
    {
      imports = [ desktopsModule ];
      calamares.desktops = [
        "aizome"
        "tatami"
      ];
      calamares.defaultDesktop = "aizome";
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        description = "Demo";
        password = "demo";
      };
      services.greetd.settings.default_session.command =
        lib.mkForce "env AIZOME_KIT_INSPECT=/run/greeter-inspect ${config.programs.aizome.package}/bin/aizome-greeter";
      systemd.tmpfiles.rules = [ "d /run/greeter-inspect 0755 greeter greeter -" ];
      environment.etc."test/aizome-settings.toml".source = aizomeSettings;
      # Unplugging a head: the test forces its connector's state, which
      # virtio's driver (not polled) reports to nobody, then announces it as
      # the kernel announces a hotplug, through a synthetic uevent whose
      # HOTPLUG=1 arrives as SYNTH_ARG_HOTPLUG.
      services.udev.extraRules = ''
        ACTION=="change", SUBSYSTEM=="drm", ENV{SYNTH_ARG_HOTPLUG}=="1", ENV{HOTPLUG}="1"
      '';
      virtualisation = {
        memorySize = 6144;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device '${gpu}'"
        ];
      };
      hardware.graphics.enable = true;
      environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
    };
  testScript = ''
    import json, os, shlex, subprocess, time

    INSPECT = "/run/greeter-inspect"

    def demo(command, check=True):
        script = (
            "export XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; "
            "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
            "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
            "export HYPRLAND_INSTANCE_SIGNATURE=$(ls /run/user/1000/hypr 2>/dev/null | head -n1); "
            + command
        )
        command = "su - demo -c " + shlex.quote(script)
        return machine.succeed(command) if check else machine.execute(command)

    def texts():
        out = machine.succeed(f'for f in {INSPECT}/*.json; do [ -f "$f" ] && cat "$f" && echo; done; true')
        docs = [json.loads(l) for l in out.splitlines() if l.strip()]
        return [e["text"] for d in docs for e in d.get("elements", []) if e.get("text") and e.get("visible", True)]

    def docs():
        out = machine.succeed(f'for f in {INSPECT}/*.json; do [ -f "$f" ] && cat "$f" && echo; done; true')
        return [json.loads(l) for l in out.splitlines() if l.strip()]

    def until(what, check, timeout=90):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            r = check()
            if r:
                return r
            time.sleep(0.5)
        raise Exception(f"timed out waiting for {what}")

    def greeter_up():
        machine.wait_until_succeeds("pgrep -u greeter -x aizome-greeter", timeout=180)
        until("the greeter's elements", lambda: texts())
        time.sleep(1)

    def choose(session):
        machine.send_key("f2")
        until("the Sessions menu", lambda: any(
            d.get("kind") == "menu" and any(e.get("label") == "Sessions" for e in d.get("elements", [])) for d in docs()))
        machine.send_chars(session.lower())
        machine.send_key("ret")
        until(f"Session: {session}", lambda: any(t.lower() == f"session: {session}".lower() for t in texts()))

    def log_in(session, running):
        greeter_up()
        choose(session)
        machine.send_chars("demo\n")
        machine.wait_until_succeeds(running, timeout=180)
        machine.wait_until_fails("pgrep -u greeter -x aizome-greeter", timeout=60)

    def log_out():
        machine.succeed("loginctl terminate-user demo")
        machine.wait_until_fails("pgrep -u demo", timeout=90)
        machine.succeed(f"rm -f {INSPECT}/*.json")

    def heads(name):
        # What each monitor shows, read from the GPU's two heads (PPM: the
        # test's QEMU writes no PNG), as the driver's screenshots are.
        for head in (0, 1):
            path = os.path.join(machine.out_dir, f"{name}-head{head}")
            reply = machine.send_monitor_command(f"screendump {path}.ppm gpu {head}")
            assert os.path.exists(f"{path}.ppm"), reply
            with open(f"{path}.png", "wb") as png:
                subprocess.run(["pnmtopng", f"{path}.ppm"], stdout=png, check=True)
            os.remove(f"{path}.ppm")

    def aizome_outputs():
        status, out = demo("aizomectl --json outputs", check=False)
        try:
            return {o["name"]: o for o in json.loads(out)["outputs"]} if status == 0 else {}
        except ValueError:
            return {}

    def hypr_monitors():
        # Nothing while Hyprland, just started or reloading, does not answer.
        status, out = demo("hyprctl -j monitors all", check=False)
        try:
            return {m["model"]: m for m in json.loads(out)} if status == 0 else {}
        except ValueError:
            return {}

    def both():
        def listed():
            monitors = hypr_monitors()
            return monitors if len(monitors) == 2 else None

        return until("both monitors in Hyprland", listed)

    machine.start()
    machine.wait_for_unit("display-manager.service")
    machine.succeed("grep -q connected /sys/class/drm/card*-Virtual-2/status")
    machine.succeed("su - demo -c 'install -Dm644 /etc/test/aizome-settings.toml ~/.config/aizome/settings.toml'")

    with subtest("Aizome shows the second display as Settings keeps it"):
        log_in("Aizome", "pgrep -u demo -x aizome-shell")
        until("Aizome's outputs", lambda: len(aizome_outputs()) == 2)
        outputs = aizome_outputs()
        print(json.dumps(outputs, indent=1))
        tall = outputs["Virtual-2"]
        assert tall["model"] == "Tall", tall
        assert tall["transform"] == "90", tall
        assert tall["scale"] == 1.25, tall
        assert (tall["logical"]["x"], tall["logical"]["y"]) == (0, 0), tall
        assert (outputs["Virtual-1"]["logical"]["x"], outputs["Virtual-1"]["logical"]["y"]) == (640, 0), outputs
        time.sleep(5)
        heads("aizome")
        log_out()

    with subtest("Tatami shows it the same way"):
        log_in("Tatami", "test -S /run/user/1000/hypr/*/.socket.sock")
        both()
        time.sleep(5)
        monitors = both()
        print(json.dumps(monitors, indent=1))
        heads("tatami")
        tall, wide = monitors["Tall"], monitors["Wide"]
        assert tall["name"] == "Virtual-2", tall
        assert tall["transform"] == 1, f"Tall turned {tall['transform']}, not 1 (90°) as in Aizome"
        assert tall["scale"] == 1.25, tall
        assert (tall["x"], tall["y"]) == (0, 0), tall
        assert (wide["x"], wide["y"]) == (640, 0), wide
        assert wide["transform"] == 0, wide

    with subtest("a reload keeps them"):
        demo("tatami reload")
        time.sleep(3)
        monitors = both()
        assert monitors["Tall"]["transform"] == 1, monitors["Tall"]
        assert (monitors["Wide"]["x"], monitors["Wide"]["y"]) == (640, 0), monitors["Wide"]

    with subtest("as displays change, Tatami takes Aizome's settings as they are then"):
        # Settings (in Tatami too) turns Tall the other way; a display change,
        # here Wide moved for the session, is what the helper watches.
        demo("sed -i 's/transform = \"90\"/transform = \"270\"/' ~/.config/aizome/settings.toml")
        move = 'hl.monitor({ output = "Virtual-1", mode = "1280x800@75", position = "640x800", scale = 1 })'
        demo(f"hyprctl eval {shlex.quote(move)}")
        until("Tall turned to 270°", lambda: hypr_monitors().get("Tall", {}).get("transform") == 3)
        monitors = both()
        # The reload also drops the rule that moved Wide.
        until("Wide back", lambda: (hypr_monitors().get("Wide", {}).get("x"), hypr_monitors().get("Wide", {}).get("y")) == (640, 0))
        assert (monitors["Tall"]["x"], monitors["Tall"]["y"]) == (0, 0), monitors["Tall"]
        demo("install -m644 /etc/test/aizome-settings.toml ~/.config/aizome/settings.toml")

    def kept(monitor, **change):
        # As the Displays window sends a display (displays/src/model.rs).
        settings = {
            "name": monitor["name"],
            "description": monitor["description"],
            "mode": f"{monitor['width']}x{monitor['height']}@{monitor['refreshRate']}",
            "position": f"{monitor['x']}x{monitor['y']}",
            "scale": monitor["scale"],
            "transform": monitor["transform"],
        }
        return settings | change

    with subtest("a display set up in Tatami keeps Tatami's settings, and Aizome its own"):
        monitors = both()
        change = {
            "displays": [kept(monitors["Tall"], transform=2), kept(monitors["Wide"], position="1024x0")],
            "apply": ["Virtual-2", "Virtual-1"],
        }
        demo(f"echo {shlex.quote(json.dumps(change))} | tatami displays apply")
        until("Tall upside down", lambda: hypr_monitors().get("Tall", {}).get("transform") == 2)
        log_out()
        log_in("Tatami", "test -S /run/user/1000/hypr/*/.socket.sock")
        both()
        time.sleep(5)
        monitors = both()
        assert monitors["Tall"]["transform"] == 2, monitors["Tall"]
        assert (monitors["Wide"]["x"], monitors["Wide"]["y"]) == (1024, 0), monitors["Wide"]
        log_out()
        log_in("Aizome", "pgrep -u demo -x aizome-shell")
        until("Aizome's outputs", lambda: len(aizome_outputs()) == 2)
        assert aizome_outputs()["Virtual-2"]["transform"] == "90", aizome_outputs()["Virtual-2"]
        log_out()

    with subtest("an unplugged display's rule goes"):
        # Forced off by the kernel, and announced as a hotplug. (Plugged in
        # again, virtio's head comes back without its EDID, unlike a monitor,
        # so the test stops here.)
        log_in("Tatami", "test -S /run/user/1000/hypr/*/.socket.sock")
        both()
        rules = "cat /run/user/1000/tatami/aizome-displays.lua"
        assert "Virtual-2" in demo(rules)
        connector = machine.succeed("ls -d /sys/class/drm/card*-Virtual-2").strip()
        card = connector.rsplit("-Virtual-2", 1)[0]
        machine.succeed(f"echo off > {connector}/status")
        machine.succeed(f"echo change $(cat /proc/sys/kernel/random/uuid) HOTPLUG=1 > {card}/uevent")
        until("Tall unplugged", lambda: list(hypr_monitors()) == ["Wide"])
        until("Aizome's rules without it", lambda: "Virtual-2" not in demo(rules))
        time.sleep(3)
        wide = hypr_monitors()["Wide"]
        assert (wide["x"], wide["y"]) == (1024, 0), wide
  '';
}
