# Exercise the real one-shot screenshot action owner, Mako and Satty.
# This is a component check, not exact-ISO qualification.
{
  pkgs,
  hyprland,
  tatami,
}:
let
  config = builtins.readFile ../desktop/tatami/config/mako/config;
  # Restore only the old history behavior for the negative control.
  original = config + ''

    [summary~="Screenshot saved"]
    history=1
  '';
in
pkgs.testers.runNixOSTest {
  name = "tatami-screenshot-notifications";
  globalTimeout = 240;
  nodes.machine = {
    imports = [ (pkgs.path + "/nixos/tests/common/user-account.nix") ];
    virtualisation = {
      memorySize = 3072;
      cores = 2;
      qemu.options = [ "-vga none -device virtio-gpu-pci,xres=1280,yres=720" ];
    };
    hardware.graphics.enable = true;
    programs.hyprland = {
      enable = true;
      package = hyprland;
    };
    environment.systemPackages = [
      tatami
      pkgs.mako
      pkgs.libnotify
      pkgs.satty
      pkgs.grim
      pkgs.wl-clipboard
    ];
    environment.etc."mako-fixed".text = config;
    environment.etc."mako-original".text = original;
    environment.etc."hypr-test.lua".text = ''
      hl.monitor({ output = "", mode = "1280x720@60", position = "0x0", scale = 1 })
      hl.config({ animations = { enabled = false }, cursor = { no_hardware_cursors = 1 } })
      hl.bind("SUPER + ALT + comma", hl.dsp.exec_cmd("makoctl invoke"))
      hl.bind("SUPER + SHIFT + ALT + comma", hl.dsp.exec_cmd("makoctl restore"))
      hl.bind("SUPER + comma", hl.dsp.exec_cmd("makoctl dismiss"))
    '';
    services.getty.autologinUser = "alice";
    programs.bash.loginShellInit = ''
      if [ "$(tty)" = /dev/tty1 ]; then
        export LIBGL_ALWAYS_SOFTWARE=1
        exec Hyprland -c /etc/hypr-test.lua > /tmp/hyprland.log 2>&1
      fi
    '';
  };
  testScript = ''
    import json, shlex, time
    from datetime import timedelta

    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock")
    signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
    environment = "export XDG_RUNTIME_DIR=/run/user/1000 HYPRLAND_INSTANCE_SIGNATURE=" + signature + "; "
    def run(command):
        return machine.succeed("su - alice -c " + shlex.quote(environment + command))
    def wait(predicate, seconds=20):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if predicate():
                return
            time.sleep(0.1)
        machine.screenshot("failure")
        print(run("journalctl --user -b --no-pager; makoctl list; makoctl history; pgrep -a tatami; pgrep -a satty || true"))
        raise AssertionError("condition did not become true")
    def instances():
        raw = run("hyprctl -j instances").strip()
        return json.loads(raw) if raw else []
    wait(lambda: any(i.get("wl_socket") for i in instances()))
    instance = instances()[0]
    pid = instance["pid"]
    wayland = instance["wl_socket"]
    environment += "export WAYLAND_DISPLAY=" + wayland + "; "
    unit_environment = " --setenv=XDG_RUNTIME_DIR=/run/user/1000 --setenv=WAYLAND_DISPLAY=" + wayland
    def start(unit, command, dbus=False):
        properties = " --property=Type=dbus --property=BusName=org.freedesktop.Notifications" if dbus else ""
        run("systemd-run --user --collect --unit=" + unit + properties + unit_environment + " " + command)
    def editor():
        return any("satty" in c["class"].lower() for c in json.loads(run("hyprctl -j clients")))
    def visible():
        return run("makoctl list")
    def history():
        return run("makoctl history")
    run("mkdir -p ~/Pictures")
    run("grim ~/Pictures/notification-test.png")
    image_hash = run("sha256sum ~/Pictures/notification-test.png")
    for variant in ("original", "fixed"):
        with subtest(variant + ": fresh Edit opens the saved image in Satty"):
            start("screenshot-mako", "mako --config /etc/mako-" + variant, dbus=True)
            machine.wait_until_succeeds("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 makoctl list'", timeout=timedelta(seconds=10))
            run("notify-send -t 1000 'Ordinary history control' 'This notice can be restored'")
            wait(lambda: "Ordinary history control" in history())
            start("fresh-" + variant, "tatami screenshot-notify /home/alice/Pictures/notification-test.png")
            wait(lambda: "Screenshot saved" in visible())
            machine.screenshot(variant + "-fresh")
            machine.send_key("meta_l-alt-comma")
            wait(editor)
            client = next(c for c in json.loads(run("hyprctl -j clients")) if "satty" in c["class"].lower())
            run("grep -zqF /home/alice/Pictures/notification-test.png /proc/" + str(client["pid"]) + "/cmdline")
            machine.screenshot(variant + "-editor")
            machine.send_key("esc")
            wait(lambda: not editor())
            wait(lambda: "inactive" in run("systemctl --user is-active fresh-" + variant + " || true"))
            assert image_hash == run("sha256sum ~/Pictures/notification-test.png")

        with subtest(variant + ": expired one-shot screenshot actions and ordinary history"):
            start("expired-" + variant, "tatami screenshot-notify /home/alice/Pictures/notification-test.png")
            wait(lambda: "Screenshot saved" in visible())
            wait(lambda: "Screenshot saved" not in visible(), seconds=15)
            wait(lambda: "inactive" in run("systemctl --user is-active expired-" + variant + " || true"))
            before = history()
            print(variant, "history after screenshot expiry:", before)
            assert ("Screenshot saved" in before) == (variant == "original")
            assert "Ordinary history control" in before
            machine.send_key("meta_l-shift-alt-comma")
            if variant == "original":
                wait(lambda: "Screenshot saved" in visible())
                machine.send_key("meta_l-alt-comma")
                time.sleep(0.5)
                assert not editor(), "negative control no longer reproduces the dead action"
                assert "Screenshot saved" in visible()
                machine.screenshot("original-restored-dead-edit")
            else:
                wait(lambda: "Ordinary history control" in visible())
                assert "Screenshot saved" not in visible()
                machine.screenshot("fixed-restored-ordinary")
                machine.send_key("meta_l-comma")
                wait(lambda: "Ordinary history control" not in visible())
                machine.send_key("meta_l-shift-alt-comma")
                wait(lambda: "Ordinary history control" in visible())
            assert image_hash == run("sha256sum ~/Pictures/notification-test.png")
            run("systemctl --user stop screenshot-mako")
            wait(lambda: run("pgrep -x mako || true").strip() == "")
    assert json.loads(run("hyprctl -j instances"))[0]["pid"] == pid
    machine.copy_from_machine("/tmp/hyprland.log")
    machine.copy_from_machine("/home/alice/Pictures/notification-test.png")
  '';
}
