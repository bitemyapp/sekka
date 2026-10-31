# Aizome's README captures, and Genkan's window in Aizome, in a disposable
# VM; no host desktop or disks are used.
#
# The VM is a Sekka system built from the same desktop module installed
# systems import, with Aizome chosen, logged in automatically as the demo
# user. Its virtio display is 2880 × 1800 and Aizome draws it at scale 2 (a
# 1440 × 900 layout), set through Aizome's own display settings path
# (`aizomectl output`). There is no GPU, so Aizome composites in software
# (pixman), its fallback. Everything is driven through `aizomectl input`,
# the compositor's own input path, as keys and clicks a person would make.
#
#     nix build .#screenshots-aizome --out-link result-screenshots-aizome
{
  pkgs,
  genkan,
  desktopsModule,
}:
let
  inherit (pkgs) lib;
  # Genkan's welcome page with sample data; preview mode exists only in
  # debug builds.
  preview = genkan.overrideAttrs {
    cargoBuildType = "debug";
    doCheck = false;
  };
  width = 2880;
  height = 1800;
  # A document for Sumi, as on Aizome's design board.
  architecture = pkgs.writeText "architecture.md" ''
    # Sidestep architecture

    ## Threads

    The handler runs on the thread that called `App::run`, and so does
    drawing, which records operations rather than touching pixels.

    ## Scale

    Operations stay in points. Each window renders at its
    output's scale: the integer one wherever it can, the
    fractional one only when asked. Every surface gets a
    buffer of `points × scale` pixels, so the compositor maps
    buffer pixels to screen pixels one to one instead of
    resampling them.

    ### Damage
    A canvas fills only the pixels whose centres its
    damage contains, so neighbouring fills share edges
    without gaps or overlaps at any scale, and a pixel
    never depends on how damage was cut up.

    - Clock tick: one rect, the clock's own.
    - Caret blink: two points wide, one line tall.
    - Window move: no raster, only composition.
  '';
in
pkgs.testers.runNixOSTest {
  name = "sekka-screenshots-aizome";
  globalTimeout = 1800;
  node.pkgsReadOnly = false;
  nodes.aizome =
    { ... }:
    {
      imports = [ desktopsModule ];
      calamares.desktops = [ "aizome" ];
      calamares.defaultDesktop = "aizome";
      networking.hostName = "sekka-demo";
      users.users.demo = {
        isNormalUser = true;
        uid = 1000;
        description = "Demo";
        password = "demo";
        extraGroups = [ "wheel" ];
      };
      services.displayManager.autoLogin = {
        enable = true;
        user = "demo";
      };
      virtualisation = {
        memorySize = 6144;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=${toString width},yres=${toString height}"
        ];
      };
      hardware.graphics.enable = true;
      # What Yukimi reads: a small channel-style configuration.
      environment.etc."nixos/configuration.nix".text = ''
        { pkgs, ... }: {
          networking.hostName = "sekka-demo";
          environment.systemPackages = [ pkgs.file pkgs.firefox ];
        }
      '';
      environment.systemPackages = [
        pkgs.libnotify
        pkgs.fastfetch
      ];
      systemd.tmpfiles.rules = [
        "d /home/demo/Documents 0755 demo users -"
        "C /home/demo/Documents/architecture.md 0644 demo users - ${architecture}"
      ];
    };
  testScript = ''
    import json, shlex, struct, subprocess, time

    def user(command):
        status, output = aizome.execute(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c "
            + shlex.quote("export AIZOME_SOCKET=$(ls /run/user/1000/aizome.*.sock | head -1); { "
                          + command + "; } 2>&1"))
        if status != 0:
            raise Exception(f"{command!r} failed with {status}:\n{output}")
        return output

    def ctl(args):
        return user("aizomectl " + args)

    def windows():
        return json.loads(ctl("--json windows")).get("windows", [])

    def wait_window(app_id):
        until = time.monotonic() + 90
        while not any(w.get("app_id") == app_id for w in windows()):
            if time.monotonic() > until:
                raise Exception(f"no {app_id} window: " + ctl("windows"))
            time.sleep(1)
        return next(w for w in windows() if w.get("app_id") == app_id)

    def launch(argv):
        ctl("spawn -- " + argv)

    def capture(name, settle=4, crop=None):
        time.sleep(settle)
        whole = name + "-display" if crop else name
        aizome.screenshot(whole)
        path = aizome.out_dir / (whole + ".png")
        with open(path, "rb") as f:
            size = struct.unpack(">II", f.read(24)[16:24])
        assert size == (${toString width}, ${toString height}), f"{whole} is {size}"
        if crop is None:
            return
        x, y, w, h = (round(v * 2) for v in crop)
        subprocess.run(
            ["${pkgs.imagemagick}/bin/magick", str(path),
             "-crop", f"{w}x{h}+{x}+{y}", "+repage",
             "-strip", "-define", "png:exclude-chunks=date,time",
             "PNG24:" + str(aizome.out_dir / (name + ".png"))],
            check=True)
        print(f"{name}.png: {w} × {h} at {x},{y} of {whole}.png")

    def frame(app_id):
        # The window with its tab, frame and hard shadow, in layout points.
        w = wait_window(app_id)
        g = w["geometry"]
        return (g["x"] - 2, g["y"] - 24, g["w"] + 8, g["h"] + 30)

    aizome.start()
    aizome.wait_until_succeeds("ls /run/user/1000/aizome.*.sock", timeout=240)
    aizome.wait_until_succeeds("pgrep -u demo -x aizome-shell", timeout=60)
    print(ctl("outputs"))
    # Scale 2, through the compositor's output settings.
    ctl("output Virtual-1 --scale 2")
    time.sleep(3)
    print(ctl("outputs"))
    # Annai's welcome greets a new user; close it as they would.
    time.sleep(6)
    for w in windows():
        if w.get("app_id") == "org.aizome.Annai":
            ctl(f"close {w['id']}")

    # The desktop, as on the design board: Sumi beside Files and Hotaru,
    # Aizome's own terminal, stacked together, the Deskbar with a
    # notification filed under its app.
    launch("aizome-hako /home/demo")
    hako = wait_window("org.aizome.Hako")
    launch("aizome-hotaru")
    hotaru = wait_window("org.aizome.Hotaru")
    ctl(f"stack {hako['id']} {hotaru['id']}")
    # Content rectangles in layout points (1440 × 900, the Deskbar at the
    # right), so every tab and frame is on screen and no tab is covered.
    ctl(f"resize {hotaru['id']} 600 500 580 340")
    time.sleep(2)
    ctl(f"focus {hotaru['id']}")
    # Hotaru's tab and status bars leave 62 × 14 cells: a short fastfetch
    # keeps its logo and title on screen with nothing wrapping.
    ctl("input type 'clear; fastfetch --logo nixos_small --structure Title:Separator:OS:Kernel:Shell:Terminal:Memory:Break:Colors'")
    ctl("input key return")
    time.sleep(3)
    launch("aizome-sumi /home/demo/Documents/architecture.md")
    sumi = wait_window("org.aizome.Sumi")
    ctl(f"resize {sumi['id']} 150 66 620 380")
    user("notify-send -a Tenki 'Rain this evening' 'Showers from 18:00, take an umbrella.'")
    capture("aizome-desktop", settle=8)

    # The whole menu at the pointer (Super + right-click), the Toolchest, and
    # the palette.
    ctl("input motion 760 300")
    ctl("input key super down")
    ctl("input click right")
    ctl("input key super up")
    capture("aizome-menu", settle=3)
    ctl("input key escape")
    time.sleep(1)
    ctl("input chord super+space")
    time.sleep(1)
    ctl("input type find")
    capture("aizome-palette", settle=3)
    ctl("input key escape")
    time.sleep(1)

    # Settings and Yukimi, each on its own.
    for w in windows():
        ctl(f"minimise {w['id']}")
    launch("aizome-settei --pane appearance")
    capture("aizome-settings", settle=8, crop=frame("org.aizome.Settei"))
    ctl(f"minimise {wait_window('org.aizome.Settei')['id']}")
    launch("aizome-yukimi")
    capture("aizome-yukimi", settle=20, crop=frame("org.aizome.Yukimi"))
    ctl(f"minimise {wait_window('org.aizome.Yukimi')['id']}")
    # Genkan, as the live media open it, on its welcome page with sample
    # data, with the pointer off its window.
    ctl("input motion 20 860")
    launch("env CALAMARES_UI_PREVIEW=welcome ${preview}/bin/calamares-nixos")
    capture("genkan", settle=8, crop=frame("org.calamares.NixOSRust"))

    print(ctl("stats"))
    aizome.shutdown()
  '';
}
