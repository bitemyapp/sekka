# Component fixture: actual Tatami launcher and file operations on a software
# GPU. Autologin and Mousepad as a disposable document producer are test-only.
{ pkgs, desktopsModule }:
pkgs.testers.runNixOSTest {
  name = "tatami-launcher-files";
  globalTimeout = 600;
  node.pkgsReadOnly = false;
  nodes.machine = { ... }: {
    imports = [ desktopsModule ];
    calamares.desktops = [ "tatami" ];
    calamares.defaultDesktop = "tatami";
    users.users.demo = {
      isNormalUser = true;
      uid = 1000;
      password = "demo";
    };
    services.displayManager.autoLogin = { enable = true; user = "demo"; };
    virtualisation = {
      memorySize = 4096;
      cores = 4;
      qemu.options = [ "-vga none" "-device virtio-gpu-pci,xres=1280,yres=800" ];
    };
    hardware.graphics.enable = true;
    environment.sessionVariables.LIBGL_ALWAYS_SOFTWARE = "1";
    environment.systemPackages = [ pkgs.mousepad ];
  };
  testScript = ''
    import json, pathlib, shlex, subprocess, time

    def user(command):
        return machine.succeed("runuser -u demo -- env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
            f"HYPRLAND_INSTANCE_SIGNATURE={signature} sh -c " + shlex.quote(command))
    def wait(predicate, label, seconds=30):
        deadline = time.monotonic() + seconds
        while time.monotonic() < deadline:
            if predicate(): return
            time.sleep(0.2)
        machine.screenshot("failure")
        raise AssertionError(label)
    def clients():
        return json.loads(user("hyprctl -j clients"))
    def client(name):
        return any(c["class"] == name for c in clients())
    def layer():
        return '"namespace": "walker"' in user("hyprctl -j layers")
    def paths(query):
        out = user("elephant query --json " + shlex.quote("files;" + query + ";1000;false"))
        items = [json.loads(line).get("item") for line in out.splitlines() if line.startswith("{")]
        return {item["text"].rstrip("/") for item in items if item}
    def screenshot(name):
        machine.screenshot(name)
        image = pathlib.Path(machine.out_dir) / (name + ".png")
        text = subprocess.check_output(["${pkgs.tesseract}/bin/tesseract", str(image), "stdout", "--psm", "11"], text=True)
        print("SCREEN", name, text)
        return text
    def search(query, expected, name, visible):
        machine.send_key("meta_l-spc")
        wait(layer, "launcher opened")
        machine.send_chars("." + query)
        time.sleep(3)
        text = screenshot(name)
        actual = paths(query)
        print("QUERY", query, actual, "EXPECTED", expected)
        assert actual == expected, (query, actual, expected)
        assert visible.casefold() in text.casefold(), (visible, text)
        machine.send_key("esc")
        wait(lambda: not layer(), "launcher closed")
    def location(path):
        machine.send_key("ctrl-l")
        machine.send_chars(path)
        machine.send_key("ret")
        time.sleep(0.5)

    machine.start()
    machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=180)
    signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
    wait(lambda: user("systemctl --user is-active tatami-elephant.service").strip() == "active", "Elephant active")
    user("systemctl --user stop tatami-hypridle.service")
    print("PRODUCTION", machine.succeed("readlink -f /run/current-system/sw/bin/elephant; sha256sum /run/current-system/sw/bin/elephant /run/current-system/sw/bin/walker; cat /etc/xdg/tatami/elephant/files.toml 2>/dev/null || true"))
    # Wait for the initial empty-home index before creating the test directory.
    time.sleep(2)
    daemon = user("systemctl --user show tatami-elephant.service -p MainPID --value").strip()
    print("LOADED", machine.succeed(f"readlink -f /proc/{daemon}/exe; sha256sum /proc/{daemon}/exe; grep files.so /proc/{daemon}/maps; cat /proc/{daemon}/cmdline"))
    machine.send_key("meta_l-shift-f")
    wait(lambda: client("org.gnome.Nautilus"), "file manager opened")
    location("/home/demo")
    machine.send_key("ctrl-shift-n")
    machine.send_chars("FreshRoot")
    machine.send_key("ret")
    machine.wait_until_succeeds("test -d /home/demo/FreshRoot")
    screenshot("01-new-root-directory")

    machine.send_key("meta_l-alt-spc")
    wait(layer, "Apps opened")
    machine.send_chars("Mousepad")
    machine.send_key("ret")
    wait(lambda: client("org.xfce.mousepad"), "document producer opened")
    machine.send_chars("Freshness fixture")
    machine.send_key("ctrl-s")
    time.sleep(0.5)
    screenshot("02-save-dialog")
    machine.send_chars("/home/demo/FreshRoot/freshnote.txt")
    machine.send_key("ret")
    machine.wait_until_succeeds("test -f /home/demo/FreshRoot/freshnote.txt")
    assert machine.succeed("cat /home/demo/FreshRoot/freshnote.txt") == "Freshness fixture"
    screenshot("03-saved-document")
    machine.send_key("meta_l-w")
    wait(lambda: not client("org.xfce.mousepad"), "document producer closed")
    search("freshnote", {"/home/demo/FreshRoot/freshnote.txt"}, "04-fresh-save-search", "freshnote.txt")

    location("/home/demo/FreshRoot")
    machine.send_key("ctrl-a")
    machine.send_key("f2")
    machine.send_chars("renamednote")
    machine.send_key("ret")
    machine.wait_until_succeeds("test -f /home/demo/FreshRoot/renamednote.txt && test ! -e /home/demo/FreshRoot/freshnote.txt")
    screenshot("05-renamed-document")
    search("freshnote", set(), "06-old-name-absent", "No matches")
    search("renamednote", {"/home/demo/FreshRoot/renamednote.txt"}, "07-new-name-search", "renamednote.txt")

    machine.send_key("alt-up")
    machine.send_key("f2")
    machine.send_chars("MovedRoot")
    machine.send_key("ret")
    machine.wait_until_succeeds("test -f /home/demo/MovedRoot/renamednote.txt && test ! -e /home/demo/FreshRoot")
    search("renamednote", {"/home/demo/MovedRoot/renamednote.txt"}, "08-moved-root-child-search", "MovedRoot")
    machine.send_key("delete")
    machine.wait_until_succeeds("test ! -e /home/demo/MovedRoot")
    screenshot("09-directory-trashed")
    search("renamednote", set(), "10-deleted-child-absent", "No matches")
    search("MovedRoot", set(), "11-deleted-directory-absent", "No matches")
    assert user("systemctl --user show tatami-elephant.service -p MainPID --value").strip() == daemon
    machine.send_key("meta_l-w")
    wait(lambda: not clients(), "test apps closed")
    screenshot("12-restored-desktop")
    print("FINAL", machine.succeed("find /home/demo/.local/share/Trash -maxdepth 5 -type f; systemctl --failed --no-legend; journalctl _UID=1000 --no-pager -p warning"))
    print("USER UNITS", user("systemctl --user --failed --no-legend"))
  '';
}
