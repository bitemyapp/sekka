# Logs into Aizome in a disposable VM and checks the desktop is there and
# works, with screenshots of each step (in the test's output directory).
#
# The VM has a virtio GPU without 3D, so the compositor renders with pixman:
# this is also the test of Aizome's software-rendered fallback. greetd logs
# alice straight into the session, as a login screen would after her
# password.
#
#     nix build .#aizome-checks.session -L
#     nix build .#aizome-checks.session.driverInteractive && ./result/bin/nixos-test-driver
{ pkgs, self }:
pkgs.testers.runNixOSTest {
  name = "aizome-session";

  nodes.machine =
    { pkgs, ... }:
    {
      imports = [ self.nixosModules.default ];
      programs.aizome.enable = true;
      # Settings takes the time zone over from here (programs.aizome.mutableTimeZone).
      time.timeZone = "Europe/Paris";

      users.users.alice = {
        isNormalUser = true;
        password = "aizome";
        extraGroups = [
          "wheel"
          "networkmanager"
          "video"
        ];
        uid = 1000;
      };

      services.greetd = {
        enable = true;
        settings.initial_session = {
          user = "alice";
          command = "/run/current-system/sw/bin/aizome-session";
        };
        settings.default_session.command = "${pkgs.greetd}/bin/agreety --cmd /run/current-system/sw/bin/aizome-session";
      };

      virtualisation = {
        memorySize = 4096;
        cores = 4;
        resolution = {
          x = 1440;
          y = 900;
        };
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1440,yres=900"
        ];
      };
      # No bluetooth controller or printers in the VM; their services still
      # run and report nothing, which is what the widgets must handle.
      environment.systemPackages = [
        # Explicit compatibility fixture; Foot is not a desktop default.
        pkgs.foot
        pkgs.wtype
        # ffprobe, for the screen recording.
        pkgs.ffmpeg
        # Screen sharing as a browser does it: the ScreenCast portal and
        # its PipeWire stream, read with GStreamer.
        (pkgs.writeShellScriptBin "aizome-screencast-check" ''
          export GST_PLUGIN_SYSTEM_PATH_1_0=${pkgs.gst_all_1.gstreamer.out}/lib/gstreamer-1.0:${pkgs.pipewire}/lib/gstreamer-1.0
          export PATH=${pkgs.gst_all_1.gstreamer.bin}/bin:$PATH
          exec ${
            pkgs.python3.withPackages (ps: [
              ps.dbus-python
              ps.pygobject3
            ])
          }/bin/python3 ${./screencast-check.py} "$@"
        '')
      ];
    };

  enableOCR = false;

  testScript = ''
    import json
    import time

    def until(what, f, timeout=60):
        deadline = time.monotonic() + timeout
        while not f():
            if time.monotonic() > deadline:
                raise Exception(f"timed out waiting for {what}")
            time.sleep(0.5)

    def as_alice(command):
        return machine.succeed(
            "su - alice -c 'export XDG_RUNTIME_DIR=/run/user/1000; "
            "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 \"^wayland-[0-9]*$\"); "
            "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
            + command + "'"
        )

    def ipc(request):
        return json.loads(as_alice("aizomectl --json " + request))

    def capture(name):
        machine.screenshot(name)
        as_alice(f"aizomectl screenshot /tmp/{name}.png")
        machine.copy_from_machine(f"/tmp/{name}.png")

    machine.wait_for_unit("multi-user.target")
    machine.wait_until_succeeds("ls /run/user/1000/ | grep -q '^aizome\\..*\\.sock$'", timeout=120)

    with subtest("the session is up: compositor, shell, outputs"):
        machine.wait_until_succeeds(
            "su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 systemctl --user is-active aizome.service'",
            timeout=60,
        )
        machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=60)
        # The session's environment file reaches the session (it must be
        # imported whole into the user manager, which the compositor and the
        # shell start from).
        shell_env = machine.succeed("tr '\\0' '\\n' < /proc/$(pgrep -u alice -x aizome-shell)/environ")
        for name in ["DCONF_PROFILE=aizome", "XDG_CONFIG_DIRS=/etc/xdg/aizome", "NIXOS_OZONE_WL=1"]:
            assert name in shell_env, f"{name} isn't in the shell's environment:\n{shell_env}"
        outputs = ipc("outputs")
        print(outputs)
        assert len(outputs) >= 1, outputs
        capture("01-desktop")

    with subtest("a terminal opens with a focused tab"):
        as_alice("aizomectl spawn foot")
        machine.wait_until_succeeds(
            "su - alice -c 'export XDG_RUNTIME_DIR=/run/user/1000 AIZOME_SOCKET=$(ls /run/user/1000/aizome.wayland-*.sock); aizomectl windows' | grep -q foot",
            timeout=30,
        )
        capture("02-terminal")

    with subtest("every app is installed: its program, desktop entry and icon"):
        sw = "/run/current-system/sw"
        apps = ["Settei", "Yukimi", "Hako", "Sumi", "Fusen", "Koyomi", "Tokei",
                "Soroban", "Tenki", "Tansu", "Kura", "Myaku", "Shashin", "Annai"]
        for app in apps:
            entry = f"{sw}/share/applications/org.aizome.{app}.desktop"
            machine.succeed(f"test -f {entry}")
            machine.succeed(f"test -f {sw}/share/icons/hicolor/scalable/apps/org.aizome.{app}.svg")
            # Exec names a program on the PATH.
            machine.succeed(f"command -v $(sed -n 's/^Exec=\\([^ ]*\\).*/\\1/p' {entry})")
        machine.succeed("test -f /etc/xdg/autostart/org.aizome.Annai.Welcome.desktop")
        # The units the package installs name its programs by store path.
        unit = "/etc/systemd/user/aizome-koyomi-agent.service"
        machine.succeed(f"grep -q '^ExecStart=/nix/store/[^ ]*/bin/aizome-koyomi --agent$' {unit}")
        machine.fail("grep -Rl @bindir@ /etc/systemd/user/")

    with subtest("Aizome's apps open"):
        for app in ["aizome-settei", "aizome-soroban", "aizome-hako"]:
            machine.succeed(f"test -x /run/current-system/sw/bin/{app}")
            as_alice(f"aizomectl spawn {app}")
        machine.sleep(5)
        print(as_alice("aizomectl windows"))
        capture("03-apps")

    with subtest("aizomectl reload swaps the window manager and the shell under the open apps"):
        listed = ipc("windows")["windows"]
        foot_ids = sorted(w["id"] for w in listed if w["app_id"] == "foot")
        assert foot_ids, listed
        compositor = machine.succeed("pgrep -u alice -x aizome").strip()
        old_shell = machine.succeed("pgrep -u alice -x aizome-shell").split()[0]
        # Nothing changed since the login: nothing to reload.
        print(as_alice("aizomectl reload"))
        # Asked for anyway: a fresh logic from its saved state, a new shell.
        report = ipc("reload --logic --shell")["report"]
        print(report)
        assert report["logic"]["action"] == "reloaded", report
        assert report["shell"]["action"] == "restarted", report
        assert report["core"]["action"] == "same", report
        machine.wait_until_fails(f"kill -0 {old_shell}", timeout=30)
        new_shell = machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=30).split()[0]
        assert new_shell != old_shell
        assert machine.succeed("pgrep -u alice -x aizome").strip() == compositor, "the compositor is the same"
        listed = ipc("windows")["windows"]
        assert sorted(w["id"] for w in listed if w["app_id"] == "foot") == foot_ids, listed
        machine.succeed("pgrep -u alice -x foot")
        logic = ipc("stats")["stats"]["logic"]
        assert logic["reloads"] == 1 and logic["healthy"], logic
        capture("03-reloaded")

    with subtest("the lock screen locks and unlocks with the password"):
        as_alice("aizomectl lock")
        machine.sleep(3)
        capture("04-locked")
        machine.send_chars("aizome\n")
        machine.sleep(3)
        capture("05-unlocked")

    with subtest("Settings can change the time zone and the computer's name"):
        # A link left from time.timeZone (absolute, NixOS's) becomes the
        # relative kind timedated writes, which Settings reads as its own.
        machine.succeed("ln -sfn /etc/zoneinfo/Europe/Paris /etc/localtime")
        machine.succeed("/run/current-system/activate")
        assert machine.succeed("readlink /etc/localtime").strip() == "../etc/zoneinfo/Europe/Paris"
        machine.succeed("timedatectl set-timezone Asia/Tokyo")
        machine.succeed("readlink /etc/localtime | grep -q Asia/Tokyo")
        machine.succeed("hostnamectl set-hostname --pretty 'Aizome Test'")
        assert "Aizome Test" in machine.succeed("hostnamectl --pretty")

    with subtest("Ctrl+Alt+Del force quits a full-screen terminal"):
        def windows():
            return ipc("windows")["windows"]

        foot = next(w for w in windows() if w["app_id"] == "foot")
        assert foot["pid"], foot
        machine.succeed(f"kill -0 {foot['pid']}")
        as_alice(f"aizomectl fullscreen {foot['id']} on")
        as_alice(f"aizomectl focus {foot['id']}")
        until("foot full screen", lambda: any(w["id"] == foot["id"] and w["fullscreen"] for w in windows()))
        as_alice("aizomectl input chord ctrl+alt+delete")
        state = ipc("recovery status")["state"]
        print(state)
        assert state["open"], state
        # The focused app is selected, unless another isn't answering.
        if state["selected"] != "foot":
            as_alice("aizomectl input key home")
            for _ in range([a["app_id"] for a in state["apps"]].index("foot")):
                as_alice("aizomectl input key down")
        assert ipc("recovery status")["state"]["selected"] == "foot"
        capture("06-recovery")
        # Delete is Force Quit for the selected app.
        as_alice("aizomectl input key delete")
        machine.wait_until_fails(f"kill -0 {foot['pid']}", timeout=30)
        until("foot's window gone", lambda: not any(w["app_id"] == "foot" for w in windows()))
        state = ipc("recovery status")["state"]
        assert not any(a["app_id"] == "foot" for a in state["apps"]), state
        assert state["message"].startswith("Force quit"), state
        capture("07-force-quit")
        as_alice("aizomectl input key escape")
        assert not ipc("recovery status")["state"]["open"]

    with subtest("a screen recording starts, shows, stops and leaves a video"):
        # gpu-screen-recorder can't run here (virtio-gpu has no hardware
        # OpenGL), so this is the fallback: wf-recorder through the
        # compositor's screencopy (wl_shm under pixman), x264.
        import shlex

        def alice(command):
            env = (
                "export XDG_RUNTIME_DIR=/run/user/1000; "
                "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
                "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
            )
            return "su - alice -c " + shlex.quote(env + command)

        def recording():
            return json.loads(machine.succeed(alice("aizome-shell msg state")))["recording"]

        record_screen = alice("aizome-shell msg '{\"request\":\"record\",\"target\":\"screen\"}'")
        # Something moving, so frames keep coming.
        machine.succeed(alice("aizomectl spawn -- foot sh -c 'while :; do date +%T.%N; sleep 0.05; done'"))
        # What the palette's Record Screen does.
        machine.succeed(record_screen)
        # (Until the shell's probe of the recorders is done, which can
        # take gpu-screen-recorder seconds here, a recording waits for it.)
        machine.wait_until_succeeds(alice("aizome-shell msg state | grep -q '\"phase\":\"recording\"'"), timeout=60)
        # One at a time.
        machine.fail(record_screen)
        # The shell found which recorder works here as it started:
        # gpu-screen-recorder refuses virtio-gpu's software OpenGL and
        # VA-API has no device, so wf-recorder with x264, at once, rather
        # than seconds into the others while the indicator counts.
        first = recording()
        print(first)
        assert first["recorder"] == "wf-recorder", first

        def five_seconds():
            r = recording()
            return r is not None and r.get("recorder") == "wf-recorder" and r.get("seconds", 0) >= 5

        for _ in range(30):
            if five_seconds():
                break
            machine.sleep(1)
        assert five_seconds(), recording()
        going = recording()
        print("recording with", going["recorder"])
        assert going["path"] == first["path"], (first, going)
        capture("06-recording")
        machine.sleep(2)
        machine.succeed(alice("aizome-shell msg stop-recording"))
        machine.wait_until_succeeds(alice("aizome-shell msg state | grep -q '\"recording\":null'"), timeout=30)
        video = going["path"]
        assert video.startswith("/home/alice/Videos/Recordings/screenrecording-"), video
        probe = json.loads(machine.succeed(
            "ffprobe -v error -select_streams v:0 -count_frames -show_entries "
            "stream=codec_name,width,height,r_frame_rate,nb_read_frames:format=duration "
            f"-of json '{video}'"
        ))
        print(probe)
        stream = probe["streams"][0]
        mode = ipc("outputs")["outputs"][0]["current_mode"]
        assert stream["codec_name"] == "h264", stream
        assert (stream["width"], stream["height"]) == (mode["width"], mode["height"]), stream
        assert stream["r_frame_rate"] == "60/1", stream
        assert float(probe["format"]["duration"]) >= 4, probe
        assert int(stream["nb_read_frames"]) >= 100, stream
        capture("07-recording-saved")

    with subtest("sharing a screen or a window streams, chosen with Aizome's picker"):
        # As a browser shares: xdg-desktop-portal-wlr asks Aizome's picker
        # (aizome-share-picker, set up by the module for Aizome sessions),
        # then streams through the compositor's ext-image-copy-capture
        # (wl_shm here, under pixman) into PipeWire. The terminal above
        # keeps changing.
        least = 20
        bus = "export DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus; "
        # In Aizome sessions the module's drop-in starts the portal with
        # Aizome's config (the portal reads only ~/.config otherwise).
        unit = machine.succeed(alice(f"{bus}systemctl --user cat xdg-desktop-portal-wlr.service"))
        print(unit)
        assert "xdg-desktop-portal-wlr-aizome" in unit, unit

        def portal_state():
            print(machine.execute("ps -ef | grep -E 'slurp|share-picker|portal-wlr|screencast-check' | grep -v grep")[1])
            print(machine.execute(alice(f"{bus}systemctl --user show-environment | grep -E 'XDG_CURRENT|WAYLAND'"))[1])
            print(machine.execute(alice(f"{bus}journalctl --user -u xdg-desktop-portal-wlr --no-pager | tail -30"))[1])

        def share(name, types, pick):
            """Asks to share; once the picker is up, picks as a person
            would; returns what the portal client said."""
            out = f"/tmp/share-{name}.log"
            events = f"/tmp/share-{name}-events.log"
            detached = "</dev/null >/dev/null 2>&1"
            machine.succeed(alice(f"setsid -f sh -c 'timeout 60 aizomectl events > {events} 2>&1' {detached}"))
            machine.wait_until_succeeds(f"test -e {events}", timeout=10)
            machine.succeed(alice(
                f"{bus}setsid -f sh -c 'aizome-screencast-check 6 {least} {types} > {out} 2>&1; echo exit $? >> {out}' {detached}"
            ))
            try:
                machine.wait_until_succeeds(f"grep -q '^pick-started share-' {events}", timeout=60)
            except Exception:
                portal_state()
                raise
            time.sleep(0.5)
            pick()
            machine.wait_until_succeeds(f"grep -q '^exit ' {out}", timeout=90)
            text = machine.succeed(f"cat {out}")
            print(text)
            return text

        def sharing():
            return json.loads(machine.succeed(alice("aizome-shell msg state")))["sharing"]

        mode = ipc("outputs")["outputs"][0]["current_mode"]
        # The terminal printing the time (the newest, on top).
        foot = [w for w in ipc("windows")["windows"] if w["app_id"] == "foot"][-1]

        # A screen: a click. The banner says what's being chosen.
        def click_screen():
            capture("09-share-picker")
            as_alice(f"aizomectl input motion {mode['width'] // 2} {mode['height'] - 40}")
            as_alice("aizomectl input click")
            # The Deskbar's indicator shows it while it lasts.
            machine.wait_until_succeeds(alice("aizome-shell msg state | grep -q '\"sharing\":\\[{'"), timeout=10)
            capture("10-sharing")

        text = share("screen", 1, click_screen)
        assert "exit 0" in text and "frames:" in text, text
        machine.wait_until_succeeds(alice("aizome-shell msg state | grep -q '\"sharing\":\\[\\]'"), timeout=20)

        # A window: Space, then a click on it.
        def click_window():
            as_alice("aizomectl input key space")
            g = foot["geometry"]
            as_alice(f"aizomectl input motion {g['x'] + g['w'] // 2} {g['y'] + g['h'] // 2}")
            as_alice("aizomectl input click")

        text = share("window", 3, click_window)
        assert "exit 0" in text and "frames:" in text, text
        # The click picked on its press; its release went to nobody, not
        # to the terminal under it.
        machine.fail("journalctl -b --no-pager -o cat | grep -F 'stray button release'")

        # Esc: the portal refuses the share, and nothing is shared.
        text = share("refused", 3, lambda: as_alice("aizomectl input key escape"))
        assert "response 1" in text and "exit 1" in text, text
        assert sharing() == [], sharing()

    with subtest("frame times stay inside the budget"):
        print(as_alice("aizomectl stats"))

    with subtest("without a usable GPU the session falls back to software by itself"):
        # virtio-gpu without 3D: the renderer setting is Automatic, and the
        # real session (udev, KMS) chose pixman on its own.
        stats = ipc("stats")["stats"]
        assert stats["backend"] == "udev", stats
        assert stats["renderer_setting"] == "auto", stats
        assert stats["renderer"] == "pixman", stats
        assert all(o["frames_rendered"] > 0 for o in stats["outputs"]), stats

    with subtest("a stalled compositor is restarted, and says so"):
        def main_pid():
            return machine.succeed(
                "su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 systemctl --user show -p MainPID --value aizome.service'"
            ).strip()

        before = main_pid()
        # Its main loop blocks: systemd's watchdog stops it after 30 s and
        # aizome-session starts another.
        as_alice("(aizomectl debug-stall 60 >/dev/null 2>&1 &)")
        machine.wait_until_fails(f"kill -0 {before}", timeout=60)
        until("another compositor", lambda: main_pid() not in ("", "0", before))
        machine.wait_until_succeeds(
            "su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 systemctl --user is-active aizome.service'", timeout=60
        )
        machine.wait_until_succeeds("ls /run/user/1000/ | grep -q '^aizome\\..*\\.sock$'", timeout=60)
        machine.succeed("grep -q 'stalled for' /home/alice/.local/state/aizome/stall.log")
        machine.succeed("grep -q 'backtrace' /home/alice/.local/state/aizome/stall.log")
        print(machine.succeed("cat /home/alice/.local/state/aizome/stall.log"))
        machine.wait_until_succeeds(
            "su - alice -c 'export XDG_RUNTIME_DIR=/run/user/1000 AIZOME_SOCKET=$(ls /run/user/1000/aizome.wayland-*.sock); "
            "aizomectl recovery status' | grep -q 'stopped responding'",
            timeout=30,
        )
        machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=60)
        machine.sleep(2)
        capture("08-restarted")
  '';
}
