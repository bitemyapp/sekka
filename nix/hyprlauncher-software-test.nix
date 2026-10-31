# Component test: actual launcher input on software graphics, not ISO qualification.
{ pkgs }:
let
  probe = pkgs.makeDesktopItem {
    name = "sekka-launcher-probe";
    desktopName = "Sekka Launcher Probe";
    exec = "${pkgs.foot}/bin/foot --app-id=sekka-launcher-probe";
    icon = "utilities-terminal";
  };
  # Keep the compositor/Aquamarine dependencies identical. The control removes
  # only this Hyprtoolkit patch and rebuilds its launcher against that library.
  unpatchedToolkit = pkgs.hyprtoolkit.overrideAttrs (old: {
    patches = pkgs.lib.remove ./patches/hyprtoolkit-buffer-allocation.patch (old.patches or [ ]);
  });
  unpatchedLauncher = pkgs.hyprlauncher.override { hyprtoolkit = unpatchedToolkit; };
  node = kernelPackages: launcher: {
    imports = [ (pkgs.path + "/nixos/tests/common/user-account.nix") ];
    boot.kernelPackages = kernelPackages;
    virtualisation.memorySize = 3072;
    virtualisation.cores = 2;
    virtualisation.qemu.options = [
      "-vga none"
      "-device virtio-gpu-pci,xres=1280,yres=800"
    ];
    hardware.graphics.enable = true;
    programs.hyprland.enable = true;
    programs.hyprland.package = pkgs.hyprland;
    environment.systemPackages = [
      launcher
      pkgs.foot
      probe
    ];
    environment.etc."hypr-test.lua".text = ''
      -- Hyprland 0.57 filters non-KMS formats by default. Keep advertising AR24
      -- so this fixture exercises the toolkit's alpha-buffer allocation fallback.
      hl.config({ quirks = { skip_non_kms_dmabuf_formats = false } })
      hl.monitor({ output = "", mode = "1280x800@60", position = "auto", scale = 1 })
      hl.bind("SUPER + R", hl.dsp.exec_cmd("env AQ_TRACE=1 ${launcher}/bin/hyprlauncher --verbose >> /tmp/launcher.log 2>&1"), { description = "Launcher regression" })
    '';
    services.getty.autologinUser = "alice";
    programs.bash.loginShellInit = ''
      if [ "$(tty)" = /dev/tty1 ]; then
        export LIBGL_ALWAYS_SOFTWARE=1
        exec Hyprland -c /etc/hypr-test.lua > /tmp/hyprland.log 2>&1
      fi
    '';
  };
in
pkgs.testers.runNixOSTest {
  name = "sekka-hyprlauncher-software";
  globalTimeout = 600;
  enableOCR = true;
  nodes.lts = node pkgs.linuxPackages pkgs.hyprlauncher;
  nodes.latest = node pkgs.linuxPackages_latest pkgs.hyprlauncher;
  nodes.original = node pkgs.linuxPackages unpatchedLauncher;
  testScript = ''
    import json
    import math
    import os
    from pathlib import Path
    import shlex
    import subprocess
    import time

    os.environ["OMP_THREAD_LIMIT"] = "1"

    def user(command):
        return "su - alice -c " + shlex.quote(
            "export XDG_RUNTIME_DIR=/run/user/1000; "
            "export HYPRLAND_INSTANCE_SIGNATURE=$(basename /run/user/1000/hypr/*); "
            + command
        )

    def until(check, description, timeout=30):
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if check():
                return
            time.sleep(0.1)
        raise AssertionError(description)

    def query(machine, name):
        return json.loads(machine.succeed(user("hyprctl -j " + name)))

    def layer(machine):
        found = [item for monitor in query(machine, "layers").values()
                 for items in monitor["levels"].values() for item in items
                 if item["namespace"] == "hyprlauncher"]
        assert len(found) <= 1, found
        return found[0] if found else None

    def monitor(machine):
        monitors = query(machine, "monitors")
        assert len(monitors) == 1, monitors
        return monitors[0]

    def start(machine):
        machine.start()
        machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=60)
        machine.wait_until_succeeds(user("hyprctl -j binds | grep -q 'Launcher regression'"), timeout=30)
        until(lambda: math.isclose(monitor(machine)["scale"], 1), "initial scale")

    def visible_text(machine, result=False):
        box = layer(machine)
        if not box:
            return ""
        output = monitor(machine)
        scale = output["scale"]
        # Read the default input field or first result within the actual layer.
        # Full-screen sparse OCR misses the small grey placeholder at scale 1;
        # reading the result also avoids confusing the text caret with a letter.
        x = round((box["x"] - output["x"] + 5) * scale)
        y = round((box["y"] - output["y"] + (42 if result else 5)) * scale)
        width = round((box["w"] - 10) * scale)
        height = round(24 * scale)
        filename = machine.name + "-ocr-screen.png"
        machine.screenshot(filename)
        screen = Path(machine.out_dir) / filename
        cropped = Path(machine.out_dir) / (machine.name + "-ocr-region.png")
        subprocess.run(["${pkgs.imagemagick}/bin/magick", str(screen), "-crop",
                        f"{width}x{height}+{x}+{y}", "+repage", "-resize", "400%",
                        "-colorspace", "Gray", "-negate", "-normalize", str(cropped)],
                       check=True, timeout=10)
        return subprocess.check_output(["${pkgs.tesseract}/bin/tesseract", str(cropped),
                                        "stdout", "--psm", "7"], text=True, timeout=10)

    def text(machine, expected):
        last = ""
        def matches():
            nonlocal last
            last = visible_text(machine, result=expected != "Search")
            return expected in last
        try:
            until(matches, "visible launcher text: " + expected)
        finally:
            print(machine.name + " OCR: " + repr(last))

    def open_launcher(machine):
        machine.send_key("meta_l-r")
        until(lambda: layer(machine), "launcher layer did not open")
        text(machine, "Search")

    def cancel(machine):
        machine.send_key("esc")
        until(lambda: layer(machine) is None, "launcher layer did not close")
        machine.succeed("pgrep -u alice -x hyprlauncher")

    def collect(machine):
        machine.screenshot(machine.name + "-final.png")
        for name in ("monitors", "layers", "clients"):
            value = query(machine, name)
            print(machine.name + " " + name + "=" + json.dumps(value))
            (Path(machine.out_dir) / (machine.name + "-" + name + ".json")).write_text(json.dumps(value))
        for name in ("launcher", "hyprland"):
            print(machine.execute("tail -100 /tmp/" + name + ".log"))
            machine.copy_from_machine("/tmp/" + name + ".log", machine.name)
        print(machine.execute("coredumpctl --no-pager info"))
        print(machine.succeed("uname -r; ls -l /dev/dri"))

    for machine in (lts, latest):
        start(machine)
        try:
            with subtest(machine.name + ": launch, search and open a desktop application"):
                open_launcher(machine)
                daemon_pid = machine.succeed("pgrep -u alice -x hyprlauncher").strip()
                machine.send_chars("Sekka Launcher Probe")
                text(machine, "Sekka Launcher Probe")
                machine.screenshot(machine.name + "-launcher-search.png")
                machine.send_key("ret")
                until(lambda: any(c["class"] == "sekka-launcher-probe" for c in query(machine, "clients")),
                      "the selected Foot application did not map")
                until(lambda: layer(machine) is None, "launcher did not close after launch")
                machine.screenshot(machine.name + "-application-opened.png")

            with subtest(machine.name + ": resize the visible launcher, cancel and reopen"):
                open_launcher(machine)
                for scale in (1.25, 1):
                    machine.send_chars("Sekka Launcher Probe")
                    text(machine, "Sekka Launcher Probe")
                    machine.succeed(user("hyprctl eval " + shlex.quote(
                        'hl.monitor({ output = "", mode = "1280x800@60", position = "auto", scale = ' + str(scale) + ' })')))
                    until(lambda: math.isclose(monitor(machine)["scale"], scale), "requested scale was not applied")
                    text(machine, "Sekka Launcher Probe")
                    machine.screenshot(machine.name + "-scale-" + str(scale).replace(".", "_") + ".png")
                    cancel(machine)
                    open_launcher(machine)
                    assert machine.succeed("pgrep -u alice -x hyprlauncher").strip() == daemon_pid
                machine.screenshot(machine.name + "-reopened.png")
                cancel(machine)
                machine.succeed("test -z \"$(coredumpctl --no-pager --no-legend list hyprlauncher 2>/dev/null)\"")
                launcher_log = machine.succeed("cat /tmp/launcher.log")
                for size in ("x: 400, y: 260", "x: 500, y: 325"):
                    assert "Reconfigured a swapchain to [Vector2D: " + size + "] AR24" in launcher_log, launcher_log
        finally:
            collect(machine)
        machine.shutdown()

    with subtest("the same software graphics reproduces the unpatched toolkit crash"):
        start(original)
        original.send_key("meta_l-r")
        original.wait_until_succeeds("coredumpctl --no-pager info hyprlauncher > /tmp/original-core.txt", timeout=30)
        core = original.succeed("cat /tmp/original-core.txt")
        assert "Signal: 11 (SEGV)" in core and "CWaylandBuffer" in core, core
        original.copy_from_machine("/tmp/original-core.txt", "original")
        collect(original)
        original.shutdown()
  '';
}
