# The login screen and the TV account with nothing but a game controller
# (a virtual one, through uinput, shaped as Microsoft's Bluetooth
# controller), end to end: greetd runs aizome-greeter; the controller
# changes nothing there until it's pressed; then it chooses Alice and types
# her password on the on-screen keyboard, and her Aizome session starts.
# Settings' helper (aizome-tv-account, as root) makes the TV account with a
# PIN; the controller chooses it and presses the PIN on the PIN pad; its
# session is Chanoma alone (no shell), set up for the first time by the
# controller, in kids' mode; Log Out ends it and the login screen comes
# back.
#
#     nix build .#aizome-checks.tv -L
{
  pkgs,
  self,
  package ? null,
}:
let
  python = pkgs.python3.withPackages (p: [ p.evdev ]);
  aizome = if package != null then package else self.packages.${pkgs.stdenv.hostPlatform.system}.aizome;
  # The controller: made with uinput, pressed through a FIFO, one command
  # a line ("tap south", "tap right", "unplug", "plug").
  pad = pkgs.writeScript "tv-test-pad" ''
    #!${python}/bin/python3
    import os, time
    from evdev import AbsInfo, UInput, ecodes as e
    FIFO = "/run/tv-pad"
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
        for a in (e.ABS_X, e.ABS_Y, e.ABS_Z, e.ABS_RZ):
            ui.write(e.EV_ABS, a, 32768)
        ui.syn()
        return ui
    # As Microsoft's controllers report them: X, the left button, is BTN_X.
    names = {"south": e.BTN_SOUTH, "east": e.BTN_EAST, "guide": e.BTN_MODE, "start": e.BTN_START,
             "x": e.BTN_X, "y": e.BTN_Y, "lb": e.BTN_TL, "rb": e.BTN_TR, "select": e.BTN_SELECT}
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
    os.chmod(FIFO, 0o666)
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
                elif words[0] == "tap":
                    press(ui, words[1], True)
                    time.sleep(0.08)
                    press(ui, words[1], False)
                    time.sleep(0.3)
                else:
                    press(ui, words[1], words[0] == "down")
                print("did", line.strip(), flush=True)
  '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-tv";
  globalTimeout = 1200;
  nodes.machine =
    { lib, ... }:
    {
      imports = [ self.nixosModules.default ];
      programs.aizome.enable = true;
      programs.aizome.package = lib.mkIf (package != null) package;
      programs.aizome.greeter.enable = true;
      programs.aizome.chanoma = {
        enable = true;
        cores = [ pkgs.libretro.gambatte ];
        browser = null;
      };
      services.displayManager.defaultSession = "aizome";
      hardware.uinput.enable = true;
      users.users.alice = {
        isNormalUser = true;
        description = "Alice Kimura";
        # Four keys along the on-screen keyboard's top row of letters.
        password = "qwer";
        uid = 1000;
      };
      # What the greeter shows, as files the test reads, and what it says,
      # in the journal.
      services.greetd.settings.default_session.command = lib.mkForce (
        "${pkgs.systemd}/bin/systemd-cat -t aizome-greeter "
        + "env AIZOME_KIT_INSPECT=/run/greeter-inspect ${aizome}/bin/aizome-greeter"
      );
      systemd.tmpfiles.rules = [ "d /run/greeter-inspect 0755 greeter greeter -" ];
      # Chanoma's own account of itself, in the TV session's journal.
      environment.etc."xdg/aizome/environment".text = lib.mkAfter "RUST_LOG=info\n";
      environment.systemPackages = [ python ];
      environment.etc."tv-pad".source = pad;
      environment.etc."tv-account-helper".source = "${aizome}/libexec/aizome-tv-account";
      virtualisation = {
        memorySize = 4096;
        cores = 4;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1920,yres=1080"
        ];
      };
    };
  testScript = builtins.readFile ./vm-tv-test.py;
}
