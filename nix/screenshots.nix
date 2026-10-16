# Real captures in disposable VMs; no host desktop or disks are used.
#
# Each VM is a Sekka system built from the same desktop module installed
# systems import, logged in automatically. The virtual display is 2560 × 1600
# at scale 2: a 1280 × 800 layout drawn at twice the density, so pages can
# show the captures sharply on high-density screens. Scaling is set the way a
# person would: Tatami's Displays settings, KDE's and GNOME's own tools.
{
  pkgs,
  genkan,
  desktopsModule,
}:
let
  inherit (pkgs) lib;
  width = 2560;
  height = 1600;
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
  # Genkan's welcome page with sample data; preview mode exists only in
  # debug builds.
  preview = genkan.overrideAttrs {
    cargoBuildType = "debug";
    doCheck = false;
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
        qemu.options = [
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
      environment.systemPackages = [
        pkgs.fastfetch
        pkgs.bottom
      ];
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
    plasma =
      { ... }:
      {
        imports = [ (sekka "plasma") ];
        environment.systemPackages = [ preview ];
      };
    gnome = sekka "gnome";
  };
  testScript = ''
    import json, shlex, struct, time

    ${wifi.script}

    SIZE = (${toString width}, ${toString height})

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

    def capture(node, name, settle=6):
        time.sleep(settle)
        node.screenshot(name)
        with open(node.out_dir / (name + ".png"), "rb") as f:
            header = f.read(24)
        size = struct.unpack(">II", header[16:24])
        assert size == SIZE, f"{name} is {size}, not {SIZE}"

    # Tatami: terminals, the menu, Yukimi in the session's GTK theme, and the
    # Network and Activity windows.
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
    while not all(m["scale"] == 2 for m in monitors()):
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
    tatami.send_key("meta_l-ret")  # Tatami's terminal
    wait_windows(1)
    time.sleep(3)
    tatami.send_chars("fastfetch\n")
    time.sleep(4)
    tatami.send_key("meta_l-ret")
    wait_windows(2)
    time.sleep(3)
    tatami.send_chars("btm\n")  # tiled beside fastfetch, as Tatami lays them out
    capture(tatami, "tatami", settle=12)
    tatami.send_key("meta_l-spc")  # the Tatami menu
    capture(tatami, "tatami-menu", settle=8)
    tatami.send_key("esc")
    time.sleep(2)
    user(tatami, "pkill -x foot || true")
    # No GPU in the VM: Cairo, rather than GTK trying Vulkan first.
    launch(tatami, "env GSK_RENDERER=cairo yukimi")
    until = time.monotonic() + 60
    while not any("yukimi" in c["class"].lower() for c in json.loads(hypr("hyprctl -j clients"))):
        if time.monotonic() > until:
            raise Exception("no Yukimi window: " + hypr("hyprctl -j clients"))
        time.sleep(1)
    capture(tatami, "yukimi", settle=30)
    user(tatami, "pkill -u demo -f '[y]ukimi' || true")
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
    until = time.monotonic() + 60
    while not any("org.tatami.Network" in c["class"] for c in json.loads(hypr("hyprctl -j clients"))):
        if time.monotonic() > until:
            raise Exception("no Network window: " + hypr("hyprctl -j clients"))
        time.sleep(1)
    capture(tatami, "network", settle=10)
    user(tatami, "pkill -u demo -f '[t]atami-network' || true")
    # The Activity window, as Super+Ctrl+T and the menu start it, over a
    # terminal running bottom: it opens with the application that had the
    # focus selected. A minute of readings, every two seconds, for its graphs.
    tatami.wait_until_fails("pgrep -u demo -f '[t]atami-network'", timeout=30)
    tatami.send_key("meta_l-ret")
    wait_windows(1)
    time.sleep(3)
    tatami.send_chars("btm\n")
    time.sleep(3)
    hypr("XDG_CONFIG_DIRS=/etc/xdg/tatami:/etc/xdg tatami launch tatami-activity")
    until = time.monotonic() + 60
    while not any(c["class"] == "org.tatami.Activity" for c in windows()):
        if time.monotonic() > until:
            raise Exception("no Activity window: " + hypr("hyprctl -j clients"))
        time.sleep(1)
    capture(tatami, "activity", settle=60)
    tatami.shutdown()

    # Plasma: the live session's desktop, with Genkan's welcome page.
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
    user(plasma, "pkill -u demo -f '[k]onsole' || true; pkill -u demo -f '[d]olphin' || true")
    launch(plasma, "env CALAMARES_UI_PREVIEW=welcome calamares-nixos")
    plasma.wait_until_succeeds("pgrep -u demo -f calamares-nixos", timeout=30)
    capture(plasma, "genkan", settle=12)
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
    launch(gnome, "env GSK_RENDERER=cairo yukimi")
    gnome.wait_until_succeeds("pgrep -u demo -f '[y]ukimi'", timeout=30)
    time.sleep(20)
    launch(gnome, "nautilus --new-window")
    capture(gnome, "gnome", settle=15)
    gnome.shutdown()
  '';
}
