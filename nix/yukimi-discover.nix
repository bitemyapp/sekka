# Yukimi's Discover in a disposable VM: a search for Steam, its package's
# details and the confirmation Install for everyone asks for, captured
# without confirming. Nothing is installed; no host desktop or disks are used.
#
# The VM is a Tatami system built like an installed Sekka system: Genkan's
# desktop and application modules (so Discover offers Genkan's catalog,
# installed through calamares.applications), unfree software allowed, and its
# Nixpkgs recorded for Yukimi, which makes its own package list from it as it
# would on a new installation. The display is the screenshots' 2560 × 1600 at
# scale 2; pointer positions below are in that 1280 × 800 layout.
{
  pkgs,
  nixpkgs,
  desktopsModule,
  applicationsModule,
}:
let
  width = 2560;
  height = 1600;
in
pkgs.testers.runNixOSTest {
  name = "yukimi-discover";
  globalTimeout = 2400;
  # Sekka's desktop module adds its Aquamarine fix as a Nixpkgs overlay.
  node.pkgsReadOnly = false;
  nodes.tatami =
    { pkgs, ... }:
    {
      imports = [
        desktopsModule
        applicationsModule
      ];
      # What the installed flake passes; only Firefox is chosen here.
      _module.args = {
        applicationPkgs = pkgs;
        aiPackages = { };
        ompPackage = null;
      };
      calamares.desktops = [ "tatami" ];
      calamares.defaultDesktop = "tatami";
      calamares.applications = [ "firefox" ];
      calamares.installUser = "demo";
      networking.hostName = "sekka-demo";
      # As on installed systems: a flake's Nixpkgs, which Yukimi searches.
      nixpkgs.flake.source = nixpkgs.outPath;
      nixpkgs.config.allowUnfree = true;
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
      # The pointer: Hyprland moves it, ydotool clicks.
      programs.ydotool.enable = true;
      virtualisation = {
        # Making the package list evaluates all of Nixpkgs.
        memorySize = 12288;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=${toString width},yres=${toString height}"
        ];
      };
      hardware.graphics.enable = true;
      environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
      # What Yukimi reads: the installer's applications and unfree software.
      environment.etc."nixos/configuration.nix".text = ''
        { pkgs, ... }: {
          networking.hostName = "sekka-demo";
          nixpkgs.config.allowUnfree = true;
          calamares.applications = [ "firefox" ];
        }
      '';
    };
  testScript = ''
    import json, re, shlex, struct, subprocess, time

    SIZE = (${toString width}, ${toString height})

    def user(command):
        status, output = tatami.execute(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c "
            + shlex.quote("{ " + command + "; } 2>&1"))
        if status != 0:
            raise Exception(f"{command!r} failed with {status}:\n{output}")
        return output

    def read(name, box):
        # Text in part of a capture (x, y, width, height in the layout).
        # Tesseract takes minutes over a whole screen a dialog has dimmed.
        x, y, w, h = (n * 2 for n in box)
        part = tatami.out_dir / (name + "-part.png")
        subprocess.run(["${pkgs.imagemagick}/bin/magick", tatami.out_dir / (name + ".png"),
                        "-crop", f"{w}x{h}+{x}+{y}", "+repage", "-colorspace", "Gray", "-negate", part],
                       check=True)
        text = subprocess.run(["${pkgs.tesseract}/bin/tesseract", part, "-"],
                              capture_output=True, text=True, check=True).stdout
        part.unlink()
        return " ".join(text.split())

    def capture(name, settle=4, expect=None, box=None, timeout=60):
        time.sleep(settle)
        until = time.monotonic() + timeout
        while True:
            tatami.screenshot(name)
            with open(tatami.out_dir / (name + ".png"), "rb") as f:
                header = f.read(24)
            size = struct.unpack(">II", header[16:24])
            assert size == SIZE, f"{name} is {size}, not {SIZE}"
            if expect is None:
                return
            text = read(name, box)
            if re.search(expect, text):
                print(f"{name}: {text}")
                return
            if time.monotonic() > until:
                raise Exception(f"{name}: no {expect!r} in {text!r}")
            time.sleep(2)

    tatami.start()
    tatami.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=180)
    signature = tatami.succeed("basename /run/user/1000/hypr/*").strip()
    def hypr(command):
        return user("export HYPRLAND_INSTANCE_SIGNATURE=" + signature + "; " + command)
    tatami.wait_until_succeeds(
        "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
        "systemctl --user is-active tatami-waybar.service", timeout=120)
    def monitors():
        return json.loads(hypr("hyprctl -j monitors"))
    hypr("printf %s '{\"displays\":[{\"name\":\"Virtual-1\",\"enabled\":true,\"scale\":2}],"
         "\"main\":\"Virtual-1\",\"apply\":[\"Virtual-1\"]}' | tatami displays apply")
    until = time.monotonic() + 30
    while not all(m["scale"] == 2 for m in monitors()):
        if time.monotonic() > until:
            raise Exception("not scaled: " + json.dumps(monitors(), indent=1))
        time.sleep(1)
    tatami.wait_for_unit("ydotoold.service")
    # Nothing here waits on idleness: the screen is neither locked nor blanked.
    user("systemctl --user stop tatami-hypridle.service")

    def pointer(x, y):
        hypr("hyprctl dispatch " + shlex.quote(f"hl.dsp.cursor.move({{ x = {x}, y = {y} }})"))
        # A pixel through the pointer device, so GTK sees it arrive.
        tatami.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool mousemove -x 1 -y 0")

    def click(x, y):
        pointer(x, y)
        time.sleep(0.5)
        tatami.succeed("YDOTOOL_SOCKET=/run/ydotoold/socket ydotool click 0xC0")
        time.sleep(1)

    def hover(x, y):
        # From outside the dialog, so the tooltip is shown afresh.
        pointer(1200, 760)
        time.sleep(1)
        pointer(x, y)

    def windows():
        return json.loads(hypr("hyprctl -j clients"))

    def yukimi_window():
        return next((c for c in windows() if "yukimi" in c["class"].lower()), None)

    # As Tatami's menu starts applications; Cairo, as there is no GPU.
    user("systemd-run --user --collect --quiet env GSK_RENDERER=cairo yukimi")
    until = time.monotonic() + 60
    while yukimi_window() is None:
        if time.monotonic() > until:
            raise Exception("no Yukimi window: " + hypr("hyprctl -j clients"))
        time.sleep(1)
    # The pointer positions below are for Yukimi tiled alone.
    window = yukimi_window()
    assert (window["at"], window["size"]) == ([12, 38], [1256, 750]), json.dumps(window)
    time.sleep(15)

    # Discover: Yukimi makes its package list from the system's Nixpkgs.
    click(90, 205)
    tatami.wait_until_succeeds(
        "test -s /home/demo/.cache/yukimi/packages-${builtins.baseNameOf nixpkgs.outPath}.json",
        timeout=900)

    # A search for Steam: the catalog's Steam above the packages.
    click(779, 129)
    tatami.send_chars("steam")
    capture("discover-steam", box=(340, 190, 870, 340),
            expect=r"Applications Steam .* Packages .*steam unfree")

    # Steam's package: installed only for everyone, as the catalog does.
    click(768, 520)
    note = (380, 580, 520, 50)
    capture("steam-details", box=note, expect=r"Steam needs 32-bit graphics drivers and device rules")
    hover(448, 669)
    capture("steam-try-it", settle=3, box=(240, 690, 420, 65), expect=r"can't be tried without installing")
    hover(589, 669)
    capture("steam-install-for-me", settle=3, box=(380, 690, 420, 65), expect=r"can't be installed just for you")
    # Neither does anything: no terminal opens, the dialog stays.
    before = len(windows())
    click(448, 669)
    click(589, 669)
    time.sleep(3)
    assert len(windows()) == before, hypr("hyprctl -j clients")
    tatami.fail("pgrep -u demo -f '[n]ix (shell|profile)'")
    capture("steam-after-clicks", settle=0, box=note, expect=r"can only be installed for everyone")

    # Install for everyone asks to install Steam, the application. Cancelled.
    configuration = tatami.succeed("sha256sum /etc/nixos/configuration.nix")
    click(781, 669)
    capture("install-steam", box=(455, 285, 370, 260), expect=r"^Install Steam\? .* Cancel Install$")
    tatami.send_key("esc")
    time.sleep(2)
    tatami.fail("pgrep -f '[y]ukimi-helper|[p]kexec'")
    assert tatami.succeed("sha256sum /etc/nixos/configuration.nix") == configuration
    tatami.fail("test -e /etc/nixos/yukimi.nix")
    tatami.shutdown()
  '';
}
