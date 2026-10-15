{
  pkgs,
  hyprland,
  unpatchedHyprland,
}:
let
  inherit (pkgs) lib;
  protocols = [
    "${pkgs.wlr-protocols}/share/wlr-protocols/unstable/wlr-screencopy-unstable-v1.xml"
    # The toplevel-export protocol refers to wlr foreign-toplevel handles.
    "${pkgs.wlr-protocols}/share/wlr-protocols/unstable/wlr-foreign-toplevel-management-unstable-v1.xml"
    "${pkgs.hyprland-protocols}/share/hyprland-protocols/protocols/hyprland-toplevel-export-v1.xml"
  ];
  client = pkgs.stdenv.mkDerivation {
    name = "screencopy-sessions";
    dontUnpack = true;
    nativeBuildInputs = [
      pkgs.pkg-config
      pkgs.wayland-scanner
    ];
    buildInputs = [ pkgs.wayland ];
    buildPhase = ''
      for xml in ${lib.escapeShellArgs protocols}; do
        name=$(basename "$xml" .xml)
        wayland-scanner client-header "$xml" "$name-client-protocol.h"
        wayland-scanner private-code "$xml" "$name-protocol.c"
      done
      $CC -std=c11 -Wall -Wextra -Wno-unused-parameter -Werror -I. \
        ${../tests/screencopy-sessions.c} *-protocol.c \
        $(pkg-config --cflags --libs wayland-client) -o screencopy-sessions
    '';
    installPhase = "install -Dm755 screencopy-sessions $out/bin/screencopy-sessions";
  };
  # The same machine for both compositors. SHARED-1 is a headless output at
  # scale 2, like a HiDPI external screen: at scale 1 a region's logical and
  # pixel boxes are equal, which hid the region bug.
  node = package: {
    imports = [ (pkgs.path + "/nixos/tests/common/user-account.nix") ];
    virtualisation = {
      memorySize = 2048;
      cores = 2;
      qemu.options = [ "-vga none -device virtio-gpu-pci" ];
    };
    hardware.graphics.enable = true;
    programs.hyprland = {
      enable = true;
      inherit package;
    };
    environment.systemPackages = [
      client
      pkgs.socat
      pkgs.wev
    ];
    environment.etc."hypr-test.lua".text = ''
      hl.monitor({ output = "", mode = "1280x720@60", position = "auto", scale = 1 })
      hl.monitor({ output = "Virtual-1", disabled = true })
      hl.monitor({ output = "SHARED-1", mode = "1280x720@60", position = "0x0", scale = 2 })
      -- Hyprland flushes stdout after every line; its log file is buffered.
      hl.config({ debug = { disable_logs = false, enable_stdout_logs = true } })
    '';
    services.getty.autologinUser = "alice";
    programs.bash.loginShellInit = ''
      if [ "$(tty)" = /dev/tty1 ]; then
        export LIBGL_ALWAYS_SOFTWARE=1 HYPRLAND_TRACE=1
        exec Hyprland -c /etc/hypr-test.lua > /tmp/hyprland.log 2>&1
      fi
    '';
  };
in
pkgs.testers.runNixOSTest {
  name = "hyprland-screenshare-sessions";
  globalTimeout = 600;
  nodes = {
    fixed = node hyprland;
    original = node unpatchedHyprland;
  };
  testScript = ''
    import json, shlex, time
    from datetime import timedelta

    class Compositor:
        def __init__(self, machine):
            self.machine = machine
            machine.wait_for_unit("multi-user.target")
            try:
                machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=timedelta(seconds=45))
            except Exception:
                print(machine.succeed("cat /tmp/hyprland.log"))
                raise
            self.signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
            self.wayland = ""
            self.wayland = json.loads(self.run("hyprctl -j instances"))[0]["wl_socket"]
            self.run("hyprctl output create headless SHARED-1")
            self.wait_for(lambda: any(m["name"] == "SHARED-1" and m["scale"] == 2 and not m["disabled"]
                                      for m in json.loads(self.run("hyprctl -j monitors all"))))
            self.pid = json.loads(self.run("hyprctl -j instances"))[0]["pid"]
            self.background("events", "socat -u UNIX-CONNECT:/run/user/1000/hypr/" + self.signature
                            + "/.socket2.sock CREATE:/tmp/events")
            self.machine.wait_until_succeeds("test -e /tmp/events")
            self.units = 0

        def run(self, command):
            return self.machine.succeed("su - alice -c " + shlex.quote(
                "export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=" + self.wayland
                + " HYPRLAND_INSTANCE_SIGNATURE=" + self.signature + "; " + command))

        def background(self, unit, command):
            self.run("systemd-run --user --unit=" + unit + " --setenv=XDG_RUNTIME_DIR=/run/user/1000"
                     " --setenv=WAYLAND_DISPLAY=" + self.wayland + " --setenv=PATH=/run/current-system/sw/bin"
                     " --property=StandardOutput=file:/tmp/" + unit + ".out " + command)

        def active(self, unit):
            return self.run("systemctl --user is-active " + unit + " || true").strip() == "active"

        def wait_for(self, predicate, seconds=10, message=""):
            until = time.monotonic() + seconds
            while time.monotonic() < until:
                if predicate():
                    return
                time.sleep(0.1)
            raise AssertionError(message + "\n" + self.machine.succeed("tail -n 40 /tmp/hyprland.log"))

        def count(self, text, path="/tmp/hyprland.log"):
            return int(self.machine.succeed("grep -cF " + shlex.quote(text) + " " + path + " || true").strip())

        def sessions(self):
            return (self.count("Created screenshare session"), self.count("Destroyed screenshare session"))

        def shares(self):
            return self.count("screencastv2>>1,", "/tmp/events")

        def capture(self, arguments, hold=0):
            self.units += 1
            unit = "capture-" + str(self.units)
            self.background(unit, "screencopy-sessions " + arguments + (" --hold " + str(hold) if hold else ""))
            self.wait_for(lambda: "captured" in self.machine.succeed("cat /tmp/" + unit + ".out 2>/dev/null || true")
                          or not self.active(unit), 60, "capture " + arguments + " did not finish")
            assert "captured" in self.machine.succeed("cat /tmp/" + unit + ".out"), \
                arguments + ": " + self.run("journalctl --user -u " + unit + " --no-pager")
            return unit

        def finish(self, unit):
            self.wait_for(lambda: not self.active(unit), 30, unit + " did not exit")
            assert self.run("systemctl --user show -p Result --value " + unit).strip() == "success", \
                self.run("journalctl --user -u " + unit + " --no-pager")

    start_all()
    patched = Compositor(fixed)
    unpatched = Compositor(original)
    # A region of SHARED-1 covering all of it, as a portal region share asks:
    # 640x360 logical, 1280x720 pixels.
    share = "region SHARED-1 0 0 640 360 120"

    with subtest("negative control: unpatched Hyprland leaks a session per frame"):
        created, destroyed = unpatched.sessions()
        unpatched.finish(unpatched.capture(share))
        assert unpatched.sessions()[0] - created == 120, unpatched.sessions()
        assert unpatched.sessions()[1] == destroyed

    with subtest("negative control: unpatched Hyprland keeps a session after its client"):
        created, destroyed = unpatched.sessions()
        unpatched.finish(unpatched.capture("output SHARED-1 30"))
        time.sleep(7)  # past the patched idle timeout too
        assert unpatched.sessions() == (created + 1, destroyed), unpatched.sessions()

    with subtest("a region share is one session, ended once idle while still connected"):
        created, destroyed = patched.sessions()
        shares = patched.shares()
        unit = patched.capture(share, hold=20)
        assert patched.sessions() == (created + 1, destroyed), patched.sessions()
        patched.wait_for(lambda: patched.shares() == shares + 1, 5, "no screencast event")
        patched.wait_for(lambda: patched.sessions()[1] == destroyed + 1, 12, "idle session was kept")
        assert patched.active(unit), "the client left before its session ended"
        assert patched.shares() == shares + 1
        patched.finish(unit)
        assert patched.sessions() == (created + 1, destroyed + 1)

    with subtest("a whole-output share ends with its client"):
        created, destroyed = patched.sessions()
        patched.finish(patched.capture("output SHARED-1 30"))
        # well before the idle timeout
        patched.wait_for(lambda: patched.sessions() == (created + 1, destroyed + 1), 2, "session outlived its client")

    with subtest("a window share ends with its client"):
        patched.background("window", "wev")
        patched.wait_for(lambda: any(c["class"] == "wev" and c["mapped"] for c in json.loads(patched.run("hyprctl -j clients"))))
        address = next(c["address"] for c in json.loads(patched.run("hyprctl -j clients")) if c["class"] == "wev")
        created, destroyed = patched.sessions()
        patched.finish(patched.capture("toplevel " + hex(int(address, 16) & 0xffffffff) + " 30"))
        patched.wait_for(lambda: patched.sessions() == (created + 1, destroyed + 1), 2, "window session outlived its client")
        patched.run("systemctl --user stop window")

    with subtest("one client holds at most 32 sessions"):
        created, destroyed = patched.sessions()
        unit = patched.capture("regions SHARED-1 40", hold=3)
        now = patched.sessions()
        # idle expiry may also have ended some on a slow machine
        assert now[0] == created + 40 and (now[0] - created) - (now[1] - destroyed) <= 32, now
        patched.finish(unit)
        patched.wait_for(lambda: patched.sessions() == (created + 40, destroyed + 40), 2)

    with subtest("sessions of departed clients are not reused or kept"):
        created, destroyed = patched.sessions()
        for i in range(20):
            patched.finish(patched.capture("region SHARED-1 " + str(i * 3) + " " + str(i * 2) + " 200 100 1"))
        patched.wait_for(lambda: patched.sessions() == (created + 20, destroyed + 20), 2)

    for compositor in (patched, unpatched):
        assert json.loads(compositor.run("hyprctl -j instances"))[0]["pid"] == compositor.pid
  '';
}
