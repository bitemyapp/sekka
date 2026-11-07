# SPDX-License-Identifier: GPL-3.0-or-later
# First-boot setup in a disposable VM, as a person meets it: an image with
# no account boots to Aizome's login screen, which sets it up. Typed at
# through QEMU's keyboard, checked through the greeter's inspection files
# (its elements and their text) and the system:
#
# - nobody can log in before setup, and no password works;
# - the Mac installer's hints are where setup starts; the keyboard is
#   changed to German, the compositor types German at once;
# - the account, host name, time zone, locale and keyboard are applied,
#   setup.nix written and evaluated, the default desktop changed in
#   configuration.nix, and nothing secret is left anywhere;
# - the login screen comes back with the new person; a wrong password is
#   refused, the right one starts Aizome, which locks and unlocks;
# - logging out, the other desktop (a stand-in session named Tatami);
# - after a reboot, the login screen as usual, everything kept.
#
#     nix build .#legacyPackages.aarch64-linux.first-boot.vm-test -L
#
# `aizomePackage` gives the compositor and session; `greeterPackage` the
# login screen under test, by default the same package.
{
  pkgs,
  aizome,
  helper,
  aizomePackage ? aizome.packages.${pkgs.stdenv.hostPlatform.system}.aizome,
  greeterPackage ? aizomePackage,
  # On an Apple Silicon host (the J514S under linux-aurora), KVM's in-kernel
  # interrupt controller stalls an aarch64 guest's boot at its serial port;
  # QEMU's own GICv2 boots it.
  userspaceIrqchip ? false,
}:
let
  inherit (pkgs) lib;
  # What the Mac installer leaves on the ESP (sekka.mac.handoff/1), in its
  # canonical form: macOS was German-speaking, in Berlin, with a US keyboard.
  handoff = builtins.toJSON {
    schema = "sekka.mac.handoff/1";
    installation = "01010101-0101-0101-0101-010101010101";
    release = "sekka-j514s-4882dc33";
    source_revision = "4882dc33652612e7475f467f6373d8b607e5407c";
    root = "63d89be1-72f7-4aff-9679-09cafa7e0c2d";
    esp = "910de5ca-7390-4716-9a54-8028916e4d78";
    created = "2026-11-06T21:00:00Z";
    hints = {
      locale = "de_DE.UTF-8";
      keyboard = "us";
      timezone = "Europe/Berlin";
      computer_name = "Ada's MacBook Pro";
      full_name = "Ada Lovelace";
    };
  };
  # A stand-in for the installed flake, without inputs so it evaluates
  # offline here: like the image's, it names its configuration after
  # setup.nix's host name, and it reads all of setup.nix.
  flake = pkgs.writeText "flake.nix" ''
    {
      outputs = { self }:
        let
          setup = import ./setup.nix { };
        in
        {
          nixosConfigurations.''${setup.networking.hostName}.config.system.build.toplevel = derivation {
            name = "first-boot-setup-check";
            system = "${pkgs.stdenv.hostPlatform.system}";
            builder = "/bin/sh";
            setup = builtins.toJSON setup;
          };
        };
    }
  '';
  # Tatami's own session builds eight first-party packages; the greeter's
  # choice of it is what's tested here, so a stand-in session has its name.
  tatami = pkgs.writeTextFile {
    name = "tatami-stand-in-session";
    destination = "/share/wayland-sessions/tatami.desktop";
    text = ''
      [Desktop Entry]
      Name=Tatami
      Comment=A stand-in for Tatami in this test: sway
      Exec=${pkgs.sway}/bin/sway
      Type=Application
      DesktopNames=Tatami
    '';
    derivationArgs.passthru.providedSessions = [ "tatami" ];
  };
in
pkgs.testers.runNixOSTest {
  name = "first-boot-setup";

  nodes.machine =
    { config, ... }:
    {
      imports = [
        aizome.nixosModules.default
        ./module.nix
      ];
      programs.aizome = {
        enable = true;
        package = aizomePackage;
        greeter.enable = true;
      };
      services.displayManager.defaultSession = "aizome";
      services.displayManager.sessionPackages = [ tatami ];
      programs.sway.enable = true;
      environment.variables.WLR_RENDERER = "pixman";
      environment.variables.WLR_NO_HARDWARE_CURSORS = "1";

      # The image: no account, no host name (the test framework names its
      # machines), and setup on.
      users.mutableUsers = true;
      networking.hostName = lib.mkForce "";
      calamares.firstBootSetup = {
        enable = true;
        package = helper;
        device = "Mac";
        desktops = [
          "aizome"
          "tatami"
        ];
        handoff = "/etc/sekka/install.json";
      };
      environment.etc."sekka/install.json".text = handoff;
      systemd.tmpfiles.rules = [
        "d /etc/nixos 0755 root root -"
        "C /etc/nixos/flake.nix 0644 root root - ${flake}"
        # The image's configuration.nix as far as setup changes it, but
        # naming Tatami, so that choosing Aizome shows setup's edit.
        "C /etc/nixos/configuration.nix 0644 root root - ${./test-data/configuration.after.nix}"
        "d /run/greeter-inspect 0755 greeter greeter -"
      ];
      nix.settings.experimental-features = [
        "nix-command"
        "flakes"
      ];

      # What the greeter shows, as files the test reads.
      services.greetd.settings.default_session.command = lib.mkForce "env AIZOME_KIT_INSPECT=/run/greeter-inspect ${greeterPackage}/bin/aizome-greeter";

      virtualisation = {
        memorySize = 4096;
        # Fewer virtual processors than a busy host has free: a starved
        # aarch64 guest can stop at boot with a watchdog lockup.
        cores = 2;
        qemu.options = [
          "-vga none"
          "-device virtio-gpu-pci,xres=1920,yres=1200"
        ]
        ++ lib.optionals userspaceIrqchip [
          "-machine"
          "kernel_irqchip=off,gic-version=2"
        ];
      };
    };

  testScript = ''
    import datetime, json, shlex, time

    INSPECT = "/run/greeter-inspect"
    # Typed with QEMU's (US) key codes once the keyboard is German, where
    # the key that types y on a US keyboard types z: the password is what
    # the German layout makes of these keys, and only that layout logs in.
    KEYS = "rainy mountain river"
    PASSWORD = "rainz mountain river"

    def docs():
        out = machine.succeed(
            f'for f in {INSPECT}/*.json; do [ -f "$f" ] && cat "$f" && echo; done; true'
        )
        return [json.loads(l) for l in out.splitlines() if l.strip()]

    def texts():
        return [e["text"] for d in docs() for e in d.get("elements", []) if e.get("text") and e.get("visible", True)]

    def until(what, check, timeout=60):
        deadline = time.monotonic() + timeout
        last = None
        while time.monotonic() < deadline:
            try:
                last = check()
            except Exception as e:
                last = e
            if last:
                return last
            time.sleep(0.5)
        focused = [e.get("name") or e.get("text") for d in docs() for e in d.get("elements", []) if e.get("focused")]
        print(machine.execute("journalctl -b --no-pager -o cat -u greetd -u sekka-setup | tail -40")[1])
        raise Exception(f"timed out waiting for {what}: {last!r}; the greeter shows {texts()}, focused {focused}")

    def focused():
        return [e.get("name") for d in docs() for e in d.get("elements", []) if e.get("focused")]

    def shows(text, timeout=60):
        return until(f"{text!r} on the login screen", lambda: any(text in t for t in texts()), timeout)

    def gone(text):
        return until(f"{text!r} gone", lambda: not any(text in t for t in texts()))

    def menu_open(title):
        return until(
            f"the {title} menu",
            lambda: any(d.get("kind") == "menu" and any(e.get("label") == title for e in d.get("elements", [])) for d in docs()),
        )

    def key(k, n=1):
        for _ in range(n):
            machine.send_key(k)
            time.sleep(0.3)

    # Unhurried: with QEMU's own interrupt controller (userspaceIrqchip) a
    # loaded host can drop keys sent 10 ms apart.
    def typed(text):
        machine.send_chars(text, delay=datetime.timedelta(milliseconds=80))

    # A key that moves to another page, pressed again if the page didn't
    # move (a key the guest never got), never once it has.
    # A key that opens a menu, again if it didn't open.
    def open_menu(k, title):
        for _ in range(3):
            key(k)
            try:
                return until(f"the {title} menu", lambda: any(d.get("kind") == "menu" and any(e.get("label") == title for e in d.get("elements", [])) for d in docs()), 15)
            except Exception as e:
                last = e
        raise last

    def advance(k, *wanted):
        for _ in range(3):
            key(k)
            try:
                return until(f"one of {wanted}", lambda: any(w in t for w in wanted for t in texts()), 15)
            except Exception as e:
                last = e
        raise last

    def greeter_up():
        machine.wait_until_succeeds("pgrep -u greeter -x aizome-greeter", timeout=120)
        machine.wait_until_succeeds("pgrep -u greeter -x aizome", timeout=60)
        until("the greeter's elements", lambda: any(t for t in texts()))
        time.sleep(1)

    def as_ada(command):
        return machine.succeed(
            "su - ada -c 'export XDG_RUNTIME_DIR=/run/user/1000; "
            "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 \"^wayland-[0-9]*$\"); "
            "export AIZOME_SOCKET=/run/user/1000/aizome.$WAYLAND_DISPLAY.sock; "
            + command + "'"
        )

    def log_out(program):
        session = machine.succeed("loginctl show-user ada -p Display --value").strip()
        machine.succeed(f"loginctl terminate-session {shlex.quote(session)}")
        machine.wait_until_fails(f"pgrep -u ada -x {program}", timeout=60)
        machine.succeed(f"rm -f {INSPECT}/*.json")

    machine.start()
    machine.wait_for_unit("greetd.service")

    with subtest("the image has no account and no usable password"):
        # Nix's build users (30001 up) have no shell; no one else is there.
        machine.succeed("! awk -F: '$3 >= 1000 && $3 < 60000 && $7 !~ /nologin$/' /etc/passwd | grep .")
        usable = machine.succeed("awk -F: '$2 !~ /^[!*]/ && $2 != \"\" {print $1}' /etc/shadow").strip()
        assert usable == "", f"accounts with a password: {usable}"
        assert machine.succeed("stat -c '%U %G %a' /run/sekka-setup.sock").split() == ["root", "sekka-setup", "660"]
        # Nobody else may ask the helper: not even a person in the group by
        # accident (SO_PEERCRED), nor anyone outside it (the socket's mode).
        machine.fail("su -s /bin/sh nobody -c 'echo {\\\"op\\\":\\\"status\\\"} | ${pkgs.socat}/bin/socat - UNIX-CONNECT:/run/sekka-setup.sock'")
        answer = machine.succeed("echo '{\"op\":\"status\"}' | ${pkgs.socat}/bin/socat -t 10 - UNIX-CONNECT:/run/sekka-setup.sock")
        assert "only the login screen" in answer, answer

    with subtest("the login screen sets the Mac up: welcome"):
        greeter_up()
        shows("Set up your Mac")
        until("Continue to have the focus", lambda: "setup-start" in focused())
        machine.screenshot("01-welcome")
        advance("ret", "Language and region")

    with subtest("language and region, from macOS's hints; the keyboard changed to German"):
        shows("Language: Deutsch (Deutschland)")
        shows("Keyboard: English (US)")
        shows("Europe/Berlin now")
        machine.screenshot("02-region")
        key("tab")
        open_menu("ret", "Keyboards")
        typed("germ")
        key("ret")
        shows("Keyboard: German")
        # The try-it field has the focus: the key that types z on a US
        # keyboard types y on a German one, once the compositor has it.
        time.sleep(2)
        typed("z")
        until("the German layout", lambda: any(t == "y" for t in texts()))
        machine.screenshot("03-german")
        # The time zone, searched: Lisbon (nothing here needs y or z).
        key("tab")
        typed("lisbon")
        shows("Lisbon, Portugal")
        key("ret")
        shows("Europe/Lisbon now")
        # Return in the emptied field goes on.
        advance("ret", "Make your account")

    with subtest("the account: macOS's name; the password typed twice on the German keyboard"):
        shows("Make your account")
        until("the suggested names", lambda: "ada" in texts() and "adas-macbook-pro" in texts())
        typed(KEYS)
        shows("Long enough.")
        key("tab")
        typed(KEYS)
        machine.screenshot("04-account")
        key("ret")
        until("the computer name's field", lambda: "setup-hostname" in focused())
        advance("ret", "Choose your desktop")

    with subtest("the desktop, the review, the work"):
        shows("Choose your desktop")
        advance("ret", "Ready to set up")
        for want in ("Name: Ada Lovelace", "User name: ada", "Computer name: adas-macbook-pro",
                     "Language: Deutsch (Deutschland)", "Keyboard: German",
                     "Time zone: Lisbon, Portugal (Europe/Lisbon)", "Desktop: Aizome"):
            shows(want)
        machine.screenshot("05-review")
        advance("ret", "Setting up your Mac", "Your account is ready")
        shows("Your account is ready", timeout=300)
        shows("adas-macbook-pro")
        machine.screenshot("06-login")

    with subtest("what setup did, and nothing secret left"):
        assert machine.succeed("id -u ada").strip() == "1000"
        groups = machine.succeed("id -nG ada").split()
        assert {"users", "wheel", "networkmanager"} <= set(groups), groups
        assert machine.succeed("getent passwd ada").strip().split(":")[4] == "Ada Lovelace"
        assert machine.succeed("stat -c '%U %a' /home/ada").split() == ["ada", "700"]
        hash = machine.succeed("awk -F: '$1 == \"ada\" {print $2}' /etc/shadow").strip()
        assert hash.startswith("$6$rounds=100000$"), "Genkan's scheme"
        assert machine.succeed("hostnamectl --static").strip() == "adas-macbook-pro"
        assert machine.succeed("cat /etc/hostname").strip() == "adas-macbook-pro"
        assert machine.succeed("readlink /etc/localtime").strip().endswith("zoneinfo/Europe/Lisbon")
        assert "LANG=de_DE.UTF-8" in machine.succeed("cat /etc/locale.conf")
        assert 'layouts = ["de"]' in machine.succeed("cat /etc/aizome/keyboard.toml")
        assert 'layouts = ["de"]' in machine.succeed("cat /home/ada/.config/aizome/settings.toml")
        setup = machine.succeed("cat /etc/nixos/setup.nix")
        for want in ('users.users."ada"', 'networking.hostName = "adas-macbook-pro";',
                     'i18n.defaultLocale = "de_DE.UTF-8";', 'services.xserver.xkb.layout = "de";'):
            assert want in setup, setup
        assert "fileSystems" not in setup, "the disk is identity.nix's"
        assert "defaultDesktop" not in setup, "the default desktop is configuration.nix's"
        # Changed as Yukimi changes it: only the default desktop's string.
        configuration = machine.succeed("cat /etc/nixos/configuration.nix")
        assert configuration == open("${./test-data/configuration.nix}").read(), configuration
        marker = json.loads(machine.succeed("cat /var/lib/sekka-setup/complete"))
        assert marker["username"] == "ada" and marker["uid"] == 1000, marker
        assert marker["system"].endswith("-first-boot-setup-check.drv"), "the flake was evaluated"
        assert machine.succeed("ls /var/lib/sekka-setup").split() == ["complete", "lock"]
        # The password (as typed, and as it is) and its hash: only the hash,
        # only in /etc/shadow.
        for secret in (KEYS, PASSWORD, hash):
            found = machine.succeed(
                f"grep -rlF -- {shlex.quote(secret)} /etc /var /home /root /run /tmp /nix/.rw-store 2>/dev/null || true"
            ).split()
            found = [f for f in found if f not in ("/etc/shadow", "/etc/shadow-")]
            assert found == [], f"{secret[:6]}… found in {found}"
            assert machine.succeed(f"journalctl -b -o cat | grep -cF -- {shlex.quote(secret)} || true").strip() == "0"

    with subtest("a wrong password is refused; the right one starts Aizome"):
        shows("Ada Lovelace")
        typed("not the password\n")
        shows("That password didn't work. Try again.")
        machine.screenshot("07-wrong")
        typed(KEYS + "\n")
        machine.wait_until_succeeds("pgrep -u ada -x aizome-shell", timeout=120)
        machine.wait_until_fails("pgrep -u greeter -x aizome-greeter", timeout=60)
        time.sleep(5)
        machine.screenshot("08-aizome")
        pid = machine.succeed("pgrep -u ada -x aizome-shell | head -1").strip()
        environ = machine.succeed(f"tr '\\0' '\\n' < /proc/{pid}/environ")
        assert "LANG=de_DE.UTF-8" in environ.splitlines(), "the session speaks the chosen language"

    with subtest("the lock screen locks and unlocks with the new password"):
        as_ada("aizomectl lock")
        until("the lock screen", lambda: "lock screen" in as_ada("aizomectl input-state"), 30)
        machine.screenshot("09-locked")
        typed(KEYS + "\n")
        until("unlocked", lambda: "lock screen" not in as_ada("aizomectl input-state"), 30)

    with subtest("logging out, then the other desktop"):
        log_out("aizome-shell")
        greeter_up()
        shows("Ada Lovelace")
        open_menu("f2", "Sessions")
        typed("tat")
        key("ret")
        shows("Session: Tatami")
        typed(KEYS + "\n")
        machine.wait_until_succeeds("pgrep -u ada -x sway", timeout=120)
        time.sleep(3)
        machine.screenshot("10-tatami")
        state = machine.succeed("cat /var/lib/aizome-greeter/state.toml")
        assert 'ada = "tatami"' in state, state
        machine.succeed("loginctl terminate-user ada")
        machine.wait_until_fails("pgrep -u ada -x sway", timeout=60)

    with subtest("after a reboot: the login screen as usual, everything kept"):
        machine.shutdown()
        machine.start()
        machine.wait_for_unit("greetd.service")
        machine.succeed(f"rm -f {INSPECT}/*.json")
        greeter_up()
        shows("Ada Lovelace")
        shows("Session: Tatami")
        assert not any("Set up your Mac" in t for t in texts())
        assert machine.succeed("hostname").strip() == "adas-macbook-pro"
        assert machine.succeed("readlink /etc/localtime").strip().endswith("zoneinfo/Europe/Lisbon")
        machine.screenshot("11-after-reboot")
        # The login screen types German too: the password, typed as before.
        open_menu("f2", "Sessions")
        typed("aiy")
        key("ret")
        typed(KEYS + "\n")
        machine.wait_until_succeeds("pgrep -u ada -x aizome-shell", timeout=120)
  '';
}
