# Chanoma in a real Aizome session, with nothing but a game controller: a
# virtual one (uinput) that looks like Microsoft's Bluetooth controller.
# Chanoma starts with the session (programs.aizome.chanoma.autostart),
# finds the controller and holds it from everyone else, and the
# controller alone opens a game from Home, plays it (holding Right turns
# the game's screen dark through the real compositor; the game is zipped,
# and the games' databases name another), pauses it with the
# Home button, and stops it, keeping where it was left. The controller
# going and coming back keeps its player number. A page in the browser
# also goes home by a keyboard's Home key (kept from the page by the
# compositor) and by the pointer, through the bar along the top, stays on
# its site (a link or a script elsewhere leaves it where it was), and takes
# typing from Chanoma's keyboard. The PIN is pressed with the controller's
# buttons. The VM renders in software, so the game's frames take the
# shared-memory path.
{
  pkgs,
  self,
  package ? null,
}:
let
  # A Game Boy game whose screen is white, or black while a direction is
  # held (apps/chanoma/tests/cores.rs makes the same bytes), zipped, as
  # cartridge sets come; and the same bytes named as the Game Boy's
  # database names a game, for Chanoma to find there.
  game = pkgs.runCommand "chanoma-test-game" { nativeBuildInputs = [ pkgs.python3 ]; } ''
    mkdir -p "$out/gb"
    python3 - "$out/gb" <<'EOF'
    import sys, zipfile
    rom = bytearray(0x8000)
    rom[0x100:0x104] = bytes([0x00, 0xC3, 0x50, 0x01])
    rom[0x104:0x134] = bytes.fromhex(
        "ceed6666cc0d000b03730083000c000d0008111f8889000edccc6ee6dddd"
        "d999bbbb67636e0eecccdddc999fbbb9333e")
    rom[0x134:0x140] = b"CHANOMA TEST"
    x = 0
    for b in rom[0x134:0x14D]:
        x = (x - b - 1) & 0xFF
    rom[0x14D] = x
    code = [0xF3, 0x3E, 0xE4, 0xE0, 0x47, 0x3E, 0x20, 0xE0, 0x00, 0xF0, 0x00, 0xF0, 0x00, 0x2F,
            0xE6, 0x0F, 0x28, 0x06, 0x3E, 0x1B, 0xE0, 0x47, 0x18, 0xED, 0x3E, 0xE4, 0xE0, 0x47,
            0x18, 0xE7]
    rom[0x150:0x150 + len(code)] = bytes(code)
    entry = zipfile.ZipInfo("Chanoma Test (World).gb", date_time=(1980, 1, 1, 0, 0, 0))
    entry.compress_type = zipfile.ZIP_DEFLATED
    with zipfile.ZipFile(sys.argv[1] + "/Chanoma Test (World).zip", "w") as z:
        z.writestr(entry, bytes(rom))
    open(sys.argv[1] + "/Tetris (World) (Rev 1).gb", "wb").write(rom)
    EOF
  '';
  # A page for the controller to drive in the browser: it says in its
  # title what reached it. Its link and its O key go elsewhere (localhost
  # isn't 127.0.0.1), its I key to its own site's other page; its search
  # box (along the top) says what's typed, and sends it as `?q=`.
  page = pkgs.writeTextDir "index.html" ''
    <!doctype html>
    <title>Chanoma test page</title>
    <style>
      body { margin: 0; background: #fff; } button { position: fixed; inset: 25%; font-size: 4em; }
      a { position: fixed; left: 0; bottom: 0; width: 20%; height: 15%; font-size: 2em; }
      input { position: fixed; left: 25%; top: 6%; width: 50%; height: 10%; font-size: 2em; }
    </style>
    <form action="/"><input id="q" name="q" placeholder="Search"></form>
    <button id="b">Press me</button>
    <a href="http://localhost:8000/elsewhere">Elsewhere</a>
    <script>
      let moved = 0;
      // Told the browser is steered, a sign-in page would refuse it.
      if (navigator.webdriver) document.title += " (automated)";
      if (location.search == "?inside") document.title += " inside";
      if (location.search.startsWith("?q=")) document.title += " searched " + location.search.slice(3);
      const q = document.getElementById("q");
      q.oninput = () => { document.title = "typed " + q.value; };
      document.getElementById("b").onclick = () => { document.title = "clicked"; };
      document.onkeydown = (e) => {
        if (e.target == q) return;
        if (e.key == "o") location.href = "http://localhost:8000/elsewhere";
        else if (e.key == "i") location.href = "/?inside";
        else document.title = "key " + e.key;
      };
      document.onmousemove = () => { if (++moved > 3) document.title = "moved"; };
    </script>
  '';
  python = pkgs.python3.withPackages (p: [
    p.evdev
    p.pillow
  ]);
  # The controller: made with uinput, pressed through a FIFO, one command
  # a line ("tap south", "down right", "up right", "unplug", "plug").
  pad = pkgs.writeScript "chanoma-test-pad" ''
    #!${python}/bin/python3
    import os, time
    from evdev import AbsInfo, UInput, ecodes as e
    FIFO = "/run/chanoma-pad"
    buttons = [e.BTN_SOUTH, e.BTN_EAST, e.BTN_C, e.BTN_NORTH, e.BTN_WEST, e.BTN_Z, e.BTN_TL, e.BTN_TR,
               e.BTN_TL2, e.BTN_TR2, e.BTN_SELECT, e.BTN_START, e.BTN_MODE, e.BTN_THUMBL, e.BTN_THUMBR]
    stick = AbsInfo(value=0, min=0, max=65535, fuzz=0, flat=0, resolution=0)
    trigger = AbsInfo(value=0, min=0, max=1023, fuzz=0, flat=0, resolution=0)
    hat = AbsInfo(value=0, min=-1, max=1, fuzz=0, flat=0, resolution=0)
    caps = {
        e.EV_KEY: buttons,
        e.EV_ABS: [(e.ABS_X, stick), (e.ABS_Y, stick), (e.ABS_Z, stick), (e.ABS_RZ, stick),
                   (e.ABS_GAS, trigger), (e.ABS_BRAKE, trigger), (e.ABS_HAT0X, hat), (e.ABS_HAT0Y, hat)],
    }
    def make():
        ui = UInput(caps, name="Xbox Wireless Controller", vendor=0x045E, product=0x0B13, version=0x0509,
                    bustype=e.BUS_BLUETOOTH)
        # Sticks at rest in the middle.
        for a in (e.ABS_X, e.ABS_Y, e.ABS_Z, e.ABS_RZ):
            ui.write(e.EV_ABS, a, 32768)
        ui.syn()
        return ui
    # As Microsoft's controllers report them: X, the left button, is BTN_X.
    names = {"south": e.BTN_SOUTH, "east": e.BTN_EAST, "guide": e.BTN_MODE, "start": e.BTN_START,
             "x": e.BTN_X, "y": e.BTN_Y, "lb": e.BTN_TL, "rb": e.BTN_TR, "select": e.BTN_SELECT}
    def lean(ui, x, y):
        ui.write(e.EV_ABS, e.ABS_X, x)
        ui.write(e.EV_ABS, e.ABS_Y, y)
        ui.syn()
    hats = {"up": (e.ABS_HAT0Y, -1), "down": (e.ABS_HAT0Y, 1), "left": (e.ABS_HAT0X, -1), "right": (e.ABS_HAT0X, 1)}
    def press(ui, what, on):
        if what in hats:
            code, v = hats[what]
            ui.write(e.EV_ABS, code, v if on else 0)
        else:
            ui.write(e.EV_KEY, names[what], 1 if on else 0)
        ui.syn()
    if os.path.exists(FIFO):
        os.unlink(FIFO)
    os.mkfifo(FIFO)
    ui = make()
    print("pad ready", flush=True)
    while True:
        with open(FIFO) as f:
            for line in f:
                words = line.split()
                if not words:
                    continue
                if words[0] == "unplug":
                    ui.close()
                    ui = None
                elif words[0] == "plug":
                    ui = make()
                elif words[0] == "stick":
                    lean(ui, int(words[1]), int(words[2]))
                elif words[0] == "tap":
                    press(ui, words[1], True)
                    time.sleep(0.08)
                    press(ui, words[1], False)
                    time.sleep(0.25)
                else:
                    press(ui, words[1], words[0] == "down")
                print("did", line.strip(), flush=True)
  '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-chanoma";
  globalTimeout = 900;
  nodes.machine =
    { lib, ... }:
    {
      imports = [ self.nixosModules.default ];
      programs.aizome.enable = true;
      programs.aizome.package = lib.mkIf (package != null) package;
      programs.aizome.chanoma = {
        enable = true;
        autostart = true;
        cores = [ pkgs.libretro.gambatte ];
        browser = pkgs.chromium;
      };
      systemd.user.services.aizome-chanoma.environment.RUST_LOG = "info";
      # The page, served here: the VM has no internet.
      systemd.services.chanoma-test-page = {
        wantedBy = [ "multi-user.target" ];
        serviceConfig.ExecStart = "${pkgs.python3}/bin/python3 -m http.server 8000 --bind 127.0.0.1 --directory ${page}";
      };
      hardware.uinput.enable = true;
      users.users.alice = {
        isNormalUser = true;
        uid = 1000;
        password = "test-only";
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
          x = 1280;
          y = 800;
        };
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1280,yres=800"
        ];
      };
      services.pipewire.extraConfig.pipewire."91-test-sink"."context.objects" = [
        {
          factory = "adapter";
          args = {
            "factory.name" = "support.null-audio-sink";
            "node.name" = "test-sink";
            "media.class" = "Audio/Sink";
            "audio.position" = "FL,FR";
            "object.linger" = true;
            "priority.session" = 1009;
            "priority.driver" = 1009;
          };
        }
      ];
      environment.systemPackages = [ python ];
      environment.etc."chanoma-games".source = game;
      environment.etc."chanoma-pad".source = pad;
    };
  testScript = builtins.readFile ./vm-chanoma-test.py;
}
