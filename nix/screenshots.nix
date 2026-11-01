# Real captures in disposable VMs; no host desktop or disks are used.
#
# Each VM is a Sekka system built from the same desktop module installed
# systems import, logged in automatically. The virtual display is drawn at
# scale 2, twice the density of its layout, so pages can show the captures
# sharply on high-density screens: 2560 × 1600 for Plasma and GNOME, and
# 2880 × 1800 for Tatami, whose Displays window (880 × 800) needs the room
# under the bar. Scaling is set the way a person would: Tatami's Displays
# settings, KDE's and GNOME's own tools.
#
# A desktop is captured whole. An application is captured on its own: the
# display is captured with nothing else on it, then cropped to the window's
# rectangle, in the display's own pixels; nothing is scaled.
{
  pkgs,
  desktopsModule,
}:
let
  inherit (pkgs) lib;
  sizes = {
    tatami = {
      width = 2880;
      height = 1800;
    };
    plasma = {
      width = 2560;
      height = 1600;
    };
    gnome = {
      width = 2560;
      height = 1600;
    };
  };
  # Made-up networks for the Network window, served by simulated Wi-Fi: none
  # is a real network anywhere.
  wifi = import ./simulated-wifi.nix {
    inherit lib pkgs;
    networks = [
      {
        ssid = "Home";
        password = "correct horse";
      }
      {
        ssid = "Café Corner";
        password = "espresso-please";
      }
      { ssid = "Riverside Library"; }
      {
        ssid = "Studio 5G";
        password = "loud-guitars";
      }
    ];
  };
  sekka =
    desktop:
    { ... }:
    {
      imports = [ desktopsModule ];
      calamares.desktops = [ desktop ];
      calamares.defaultDesktop = desktop;
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
        # virtio-gpu offers the size as its preferred mode; there is no GPU,
        # so Mesa renders in software.
        qemu.options = with sizes.${desktop}; [
          "-vga none"
          "-device virtio-gpu-pci,xres=${toString width},yres=${toString height}"
        ];
      };
      hardware.graphics.enable = true;
      environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
      # What Yukimi reads: a small channel-style configuration.
      environment.etc."nixos/configuration.nix".text = ''
        { pkgs, ... }: {
          networking.hostName = "sekka-demo";
          environment.systemPackages = [ pkgs.file pkgs.firefox ];
        }
      '';
      environment.systemPackages = [ pkgs.fastfetch ];
    };
in
pkgs.testers.runNixOSTest {
  name = "sekka-screenshots";
  globalTimeout = 2400;
  # Sekka's desktop module adds its Aquamarine fix as a Nixpkgs overlay, as
  # on installed systems.
  node.pkgsReadOnly = false;
  nodes = {
    tatami =
      { ... }:
      {
        imports = [
          (sekka "tatami")
          wifi.module
        ];
        # Only Wi-Fi in the Network window: the VM's own wired links are not
        # NetworkManager's.
        networking.networkmanager.unmanaged = [ "interface-name:eth*" ];
        users.users.demo.extraGroups = [ "networkmanager" ];
      };
    plasma = sekka "plasma";
    gnome = sekka "gnome";
  };
  testScript = ''
    import json, shlex, struct, subprocess, time

    ${wifi.script}

    SIZES = {
        name: (size["width"], size["height"])
        for name, size in json.loads(${lib.escapeShellArg (builtins.toJSON sizes)}).items()
    }
    SCALE = 2

    def user(node, command):
        status, output = node.execute(
            "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus sh -c "
            + shlex.quote("{ " + command + "; } 2>&1"))
        if status != 0:
            raise Exception(f"{command!r} failed with {status}:\n{output}")
        return output

    def launch(node, command):
        # A user service, with the environment the session gave the user
        # manager: how Tatami's menu starts applications too.
        user(node, "systemd-run --user --collect --quiet " + command)

    def capture(node, name, settle=6, window=None):
        # The whole display at its native resolution. With window, a function
        # giving the window's rectangle in layout pixels, the capture is kept
        # as NAME-display.png and NAME.png is that rectangle of it.
        if node.name == "tatami":
            # Tatami hides the pointer at a key press, until it moves; Shift
            # alone does nothing in a window.
            node.send_key("shift")
        time.sleep(settle)
        whole = name + "-display" if window else name
        node.screenshot(whole)
        path = node.out_dir / (whole + ".png")
        with open(path, "rb") as f:
            header = f.read(24)
        size = struct.unpack(">II", header[16:24])
        assert size == SIZES[node.name], f"{whole} is {size}, not {SIZES[node.name]}"
        if window is None:
            return
        x, y, w, h = (round(v * SCALE) for v in window())
        assert 0 <= x and 0 <= y and x + w <= size[0] and y + h <= size[1], (
            f"{name}'s window {w}×{h}+{x}+{y} is not all on the {size} display")
        subprocess.run(
            ["${pkgs.imagemagick}/bin/magick", str(path),
             "-crop", f"{w}x{h}+{x}+{y}", "+repage",
             "-strip", "-define", "png:exclude-chunks=date,time",
             "PNG24:" + str(node.out_dir / (name + ".png"))],
            check=True)
        print(f"{name}.png: {w} × {h} at {x},{y} of {whole}.png")

    # Tatami: Activity, the menu, Displays, native Yukimi, and the Network window, each on its own.
    tatami.start()
    start_access_points(tatami)
    tatami.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=180)
    signature = tatami.succeed("basename /run/user/1000/hypr/*").strip()
    def hypr(command):
        return user(tatami, "export HYPRLAND_INSTANCE_SIGNATURE=" + signature + "; " + command)
    tatami.wait_until_succeeds(
        "runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
        "systemctl --user is-active tatami-waybar.service", timeout=120)
    # Scale 2, through Displays' own apply path.
    def monitors():
        return json.loads(hypr("hyprctl -j monitors"))
    print(json.dumps(monitors(), indent=1))
    print(hypr("printf %s '{\"displays\":[{\"name\":\"Virtual-1\",\"enabled\":true,\"scale\":2}],"
               "\"main\":\"Virtual-1\",\"apply\":[\"Virtual-1\"]}' | tatami displays apply"))
    until = time.monotonic() + 30
    while not all(m["scale"] == SCALE for m in monitors()):
        if time.monotonic() > until:
            raise Exception("not scaled: " + json.dumps(monitors(), indent=1))
        time.sleep(1)
    # From here on, the keys a person would press.
    def windows():
        return json.loads(hypr("hyprctl -j clients"))
    def wait_windows(count):
        until = time.monotonic() + 60
        while len(windows()) < count:
            if time.monotonic() > until:
                raise Exception(f"expected {count} windows: " + hypr("hyprctl -j clients"))
            time.sleep(1)
    def window(match):
        found = [c for c in windows() if match.lower() in c["class"].lower()]
        return found[0] if found else None
    def wait_window(match, gone=False):
        until = time.monotonic() + 60
        while (window(match) is None) != gone:
            if time.monotonic() > until:
                state = "a" if gone else "no"
                raise Exception(f"{state} {match} window: " + hypr("hyprctl -j clients"))
            time.sleep(1)
    def alone(match):
        # The window's rectangle, once nothing else shares its workspace.
        def rect():
            client = window(match)
            others = [c for c in windows()
                      if c["workspace"]["id"] == client["workspace"]["id"]
                      and c["address"] != client["address"]]
            assert not others, f"{match} shares its workspace: {json.dumps(others, indent=1)}"
            return (*client["at"], *client["size"])
        return rect
    def active_workspace():
        return json.loads(hypr("hyprctl -j activeworkspace"))["id"]
    tatami.send_key("meta_l-ret")  # Tatami's terminal
    wait_windows(1)
    time.sleep(3)
    # Some work for it: how small the system's software compresses, on two
    # cores, which takes longer than the captures below. Nothing is written.
    tatami.send_chars("tar -chC /run/current-system sw | xz -T2 | wc -c\n")
    tatami.wait_until_succeeds("pgrep -u demo -x xz", timeout=30)
    def working():
        status, _ = tatami.execute("pgrep -u demo -x xz")
        if status != 0:
            print(tatami.execute("df -h / /home; free -m; journalctl -k -n 20 --no-pager")[1])
            raise Exception("the terminal's compression ended before the captures")
    time.sleep(4)
    # Activity, as Super+Ctrl+T opens it: with the application that had the
    # focus, the terminal, selected. Super+Shift+2 takes it to a workspace of
    # its own, where a minute of readings, every two seconds, fill its graphs.
    tatami.send_key("meta_l-ctrl-t")
    wait_window("org.tatami.Activity")
    time.sleep(3)
    tatami.send_key("meta_l-shift-2")
    time.sleep(2)
    if active_workspace() != 2:
        tatami.send_key("meta_l-2")
    capture(tatami, "activity", settle=60, window=alone("org.tatami.Activity"))
    working()
    # Its Memory view: each application's proportional, private and
    # resident memory.
    tatami.send_key("ctrl-2")
    capture(tatami, "activity-memory", settle=10, window=alone("org.tatami.Activity"))
    working()
    user(tatami, "pkill -u demo -f '[t]atami-activity' || true")
    for client in windows():
        if client["class"] == "org.aizome.Hotaru":
            user(tatami, f"kill {client['pid']}")
    wait_window("org.tatami.Activity", gone=True)
    # The Tatami menu over an empty desktop.
    time.sleep(2)
    tatami.send_key("meta_l-spc")
    capture(tatami, "tatami-menu", settle=8)
    tatami.send_key("esc")
    time.sleep(2)
    # Displays, from Super+Ctrl+D; Super+W closes it.
    tatami.send_key("meta_l-ctrl-d")
    wait_window("org.tatami.Displays")
    capture(tatami, "displays", settle=10, window=alone("org.tatami.Displays"))
    tatami.send_key("meta_l-w")
    wait_window("org.tatami.Displays", gone=True)
    launch(tatami, "yukimi")
    wait_window("yukimi")
    capture(tatami, "yukimi", settle=30, window=alone("yukimi"))
    user(tatami, "pkill -u demo -f '[y]ukimi' || true")
    wait_window("yukimi", gone=True)
    # The Network window: joined to Home, with Café Corner and a cabin's
    # network saved and a WireGuard VPN set up (switched off).
    tatami.wait_until_succeeds(
        "nmcli -t -f SSID device wifi list --rescan yes | grep -qx 'Studio 5G'", timeout=120)
    tatami.succeed(
        "nmcli connection add type wifi con-name 'Café Corner' ssid 'Café Corner' "
        "wifi-sec.key-mgmt wpa-psk wifi-sec.psk espresso-please",
        "nmcli connection add type wifi con-name 'Mountain Cabin' ssid 'Mountain Cabin' "
        "wifi-sec.key-mgmt wpa-psk wifi-sec.psk snowed-in-again",
        "nmcli connection add type wireguard ifname wg0 con-name 'Work VPN' autoconnect no "
        "wireguard.private-key yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=",
        "nmcli --wait 90 device wifi connect Home password 'correct horse' ifname wlan1",
    )
    # As the panel, the menu and the launcher start it (`tatami launch`).
    user(tatami, "XDG_CONFIG_DIRS=/etc/xdg/tatami:/etc/xdg tatami launch tatami-network")
    wait_window("org.tatami.Network")
    capture(tatami, "network", settle=10, window=alone("org.tatami.Network"))
    tatami.shutdown()

    # Plasma.
    plasma.start()
    plasma.wait_until_succeeds("pgrep -u demo -f bin/.?plasmashell", timeout=240)
    time.sleep(10)
    user(plasma, "pkill -u demo -f '[p]lasma-welcome' || true")
    # KScreen's own tool, on the session's Wayland display.
    user(plasma, "unset DISPLAY; export QT_QPA_PLATFORM=wayland "
         "WAYLAND_DISPLAY=$(basename $(ls -d /run/user/1000/wayland-? | head -1)); "
         "kscreen-doctor output.Virtual-1.scale.2")
    # Dolphin, then Konsole running fastfetch over it, as KWin places them.
    launch(plasma, "dolphin")
    time.sleep(8)
    launch(plasma, "konsole --hold -e fastfetch")
    plasma.wait_until_succeeds("pgrep -u demo -f '[k]onsole'", timeout=30)
    capture(plasma, "plasma", settle=15)
    plasma.shutdown()

    # GNOME, after closing its first-login tour.
    gnome.start()
    gnome.wait_until_succeeds("pgrep -u demo -f bin/.?gnome-shell", timeout=240)
    # GNOME Shell's welcome dialog opens on first login: Escape, as a new
    # user would skip it.
    time.sleep(15)
    gnome.send_key("esc")
    time.sleep(3)
    user(gnome, "${pkgs.mutter}/bin/gdctl set --logical-monitor --primary --monitor Virtual-1 --scale 2")
    # Yukimi, then Files over it, as GNOME places them.
    launch(gnome, "yukimi")
    gnome.wait_until_succeeds("pgrep -u demo -f '[y]ukimi'", timeout=30)
    time.sleep(20)
    launch(gnome, "nautilus --new-window")
    capture(gnome, "gnome", settle=15)
    gnome.shutdown()
  '';
}
