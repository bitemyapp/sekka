# Pixel regression for the software cursor in actual compositor captures.
# Component proof, not qualification of a release ISO or a physical GPU.
{
  pkgs,
  hyprland,
  unpatchedHyprland,
}:
let
  # Stock grim prefers ext-image-copy-capture. Exercise its existing legacy
  # wlr-screencopy path too, changing only this test client's protocol choice.
  grimWlr = pkgs.grim.overrideAttrs (old: {
    pname = "grim-wlr-test";
    postPatch = (old.postPatch or "") + ''
      substituteInPlace main.c --replace-fail \
        'if (state->ext_output_image_capture_source_manager != NULL) {' \
        'if (false) {'
    '';
  });
  fixture = pkgs.writeShellScript "capture-fixture" ''
    printf '\033[?25lCAPTURE TEXT CHECK\n'
    exec sleep infinity
  '';
  node = package: {
    imports = [ (pkgs.path + "/nixos/tests/common/user-account.nix") ];
    virtualisation = {
      memorySize = 3072;
      cores = 2;
      qemu.options = [ "-vga none -device virtio-gpu-pci,xres=1280,yres=720" ];
    };
    hardware.graphics.enable = true;
    programs.hyprland = {
      enable = true;
      inherit package;
    };
    environment.systemPackages = [
      pkgs.foot
      pkgs.grim
      pkgs.tesseract
      pkgs.wf-recorder
      pkgs.ffmpeg-headless
    ];
    environment.etc."hypr-test.lua".text = ''
      hl.monitor({ output = "", mode = "1280x720@60", position = "0x0", scale = 1 })
      hl.config({ animations = { enabled = false }, cursor = { no_hardware_cursors = 1 },
                  misc = { disable_hyprland_logo = true, disable_splash_rendering = true, background_color = "rgb(234567)" },
                  decoration = { blur = { enabled = false } },
                  debug = { disable_logs = false, enable_stdout_logs = true } })
    '';
    services.getty.autologinUser = "alice";
    programs.bash.loginShellInit = ''
      if [ "$(tty)" = /dev/tty1 ]; then
        export LIBGL_ALWAYS_SOFTWARE=1
        # Own the cursor fixture instead of relying on Hyprland's emergency
        # 32px fallback bitmap, which cannot supply scaled theme images.
        export XCURSOR_THEME=Adwaita XCURSOR_SIZE=32
        export XCURSOR_PATH=${pkgs.adwaita-icon-theme}/share/icons
        exec Hyprland -c /etc/hypr-test.lua > /tmp/hyprland.log 2>&1
      fi
    '';
  };
in
pkgs.testers.runNixOSTest {
  name = "hyprland-capture-cursor";
  globalTimeout = 600;
  extraPythonPackages = p: [ p.pillow ];
  nodes.fixed = node hyprland;
  nodes.original = node unpatchedHyprland;
  testScript = ''
    import json, shlex, time
    from PIL import Image, ImageChops
    from datetime import timedelta

    class Compositor:
        def __init__(self, machine):
            self.machine = machine
            machine.wait_for_unit("multi-user.target")
            machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=timedelta(seconds=60))
            self.signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
            self.wayland = ""
            self.pid = 0
            # The command socket appears before the instance's lock metadata.
            # Wait for the actual API result instead of racing that startup gap.
            def instance_ready():
                raw = self.run("hyprctl -j instances")
                if not raw.strip():
                    return False
                instances = json.loads(raw)
                if len(instances) != 1 or not instances[0].get("wl_socket"):
                    return False
                self.wayland = instances[0]["wl_socket"]
                self.pid = instances[0]["pid"]
                return True
            self.wait_for(instance_ready, seconds=60)
            self.run("systemd-run --user --unit=fixture --setenv=WAYLAND_DISPLAY=" + self.wayland
                     + " --setenv=XDG_RUNTIME_DIR=/run/user/1000 foot -o colors-dark.background=1a1b26 ${fixture}")
            self.wait_for(lambda: any(c["class"] == "foot" for c in json.loads(self.run("hyprctl -j clients"))))
            self.run("hyprctl dispatch 'hl.dsp.window.fullscreen({ mode = \"fullscreen\" })'")
            self.wait_for(lambda: json.loads(self.run("hyprctl -j activewindow"))["size"] == [1280, 720])
            self.machine.succeed("grep -qi llvmpipe /tmp/hyprland.log")

        def run(self, command):
            return self.machine.succeed("su - alice -c " + shlex.quote(
                "export XDG_RUNTIME_DIR=/run/user/1000 WAYLAND_DISPLAY=" + self.wayland
                + " HYPRLAND_INSTANCE_SIGNATURE=" + self.signature + "; " + command))

        def wait_for(self, predicate, seconds=15):
            until = time.monotonic() + seconds
            while time.monotonic() < until:
                if predicate():
                    return
                time.sleep(0.1)
            self.machine.screenshot(self.machine.name + "-failure")
            raise AssertionError(self.machine.succeed("tail -50 /tmp/hyprland.log"))

        def move(self, x, y):
            self.run("hyprctl dispatch " + shlex.quote(f"hl.dsp.cursor.move({{ x = {x}, y = {y} }})"))
            self.wait_for(lambda: self.run("hyprctl cursorpos").strip() == f"{x}, {y}")
            time.sleep(0.2)

        def capture(self, protocol, name, cursor=False, output=None, region="100,100 220x180"):
            command = {"ext": "${pkgs.grim}/bin/grim", "wlr": "${grimWlr}/bin/grim"}[protocol]
            path = "/tmp/" + name + ".png"
            self.run("WAYLAND_DEBUG=1 " + command + (" -c" if cursor else "")
                     + (" -o " + output if output else " -g " + shlex.quote(region))
                     + " " + path + " 2>/tmp/" + name + ".protocol")
            expected = "ext_image_copy_capture_manager_v1" if protocol == "ext" else "zwlr_screencopy_manager_v1"
            self.machine.succeed("grep -q " + expected + " /tmp/" + name + ".protocol")
            self.machine.copy_from_machine(path, self.machine.name)
            self.machine.copy_from_machine("/tmp/" + name + ".protocol", self.machine.name)
            return Image.open(self.machine.out_dir / self.machine.name / (name + ".png")).convert("RGB")

    def changed(a, b):
        assert a.size == b.size, (a.size, b.size)
        difference = ImageChops.difference(a, b)
        return sum(pixel != (0, 0, 0) for pixel in difference.getdata()), difference.getbbox()

    def check_zoomed_cursor(tag, output, scale=1, transform=0):
        fixed.screenshot(tag + "-display")
        displayed = Image.open(fixed.out_dir / (tag + "-display.png")).convert("RGB")
        if transform:
            displayed = displayed.transpose(Image.Transpose.ROTATE_270)
        for protocol in ("ext", "wlr"):
            name = tag + "-" + protocol
            excluded = patched.capture(protocol, name + "-excluded", output=output)
            included = patched.capture(protocol, name + "-included", cursor=True, output=output)
            display_count, display_box = changed(excluded, displayed)
            capture_count, capture_box = changed(excluded, included)
            print(name, "display/capture cursor", display_count, display_box, capture_count, capture_box)
            assert display_box is not None and capture_box is not None
            assert all(abs(a - b) <= 2 for a, b in zip(display_box, capture_box)), (display_box, capture_box)
            assert 0.8 * display_count <= capture_count <= 1.2 * display_count
            assert 30 * scale < display_box[2] - display_box[0] < 100 * scale
            assert 30 * scale < display_box[3] - display_box[1] < 100 * scale
            colors = excluded.crop(display_box).getcolors(10000 * scale * scale)
            assert colors is not None and len(colors) == 1, "excluded zoomed cursor leaked"
            x, y, right, bottom = display_box
            crop = (max(0, (x - 10) // scale * scale), max(0, (y - 10) // scale * scale),
                    min(displayed.width, (right + 10) // scale * scale), min(displayed.height, (bottom + 10) // scale * scale))
            region = f"{crop[0] // scale},{crop[1] // scale} {(crop[2] - crop[0]) // scale}x{(crop[3] - crop[1]) // scale}"
            regional = patched.capture(protocol, name + "-region", cursor=True, region=region)
            assert changed(included.crop(crop), regional)[0] == 0, "capture crop was applied before zoom"

    start_all()
    baseline = Compositor(original)
    patched = Compositor(fixed)
    for protocol in ("ext", "wlr"):
        with subtest(protocol + " negative control: software cursor leaks when excluded"):
            baseline.move(600, 500)
            empty = baseline.capture(protocol, protocol + "-away")
            baseline.move(160, 150)
            leaked = baseline.capture(protocol, protocol + "-leaked")
            count, box = changed(empty, leaked)
            print(protocol, "original cursor pixels", count, box)
            assert 10 < count < 3000, (count, box)

    for scale, transform in ((1, 0), (2, 0), (1, 1)):
        patched.run("hyprctl eval " + shlex.quote(
            f'hl.monitor({{ output = "Virtual-1", mode = "1280x720@60", position = "0x0", scale = {scale}, transform = {transform} }})'))
        patched.wait_for(lambda: all(json.loads(patched.run("hyprctl -j monitors"))[0][key] == value
                                    for key, value in (("scale", scale), ("transform", transform))))
        for protocol in ("ext", "wlr"):
            tag = protocol + "-scale" + str(scale) + "-transform" + str(transform)
            with subtest(tag + ": cursor exclusion, explicit overlay, movement and blank OCR"):
                patched.move(500, 300)
                empty = patched.capture(protocol, tag + "-away")
                colors = empty.getcolors(empty.width * empty.height)
                assert colors is not None and len(colors) == 1, "fixture region is not blank"
                patched.move(160, 150)
                hidden = patched.capture(protocol, tag + "-excluded")
                assert changed(empty, hidden)[0] == 0, "excluded cursor leaked"
                shown = patched.capture(protocol, tag + "-included", cursor=True)
                count, box = changed(empty, shown)
                print(tag, "explicit cursor pixels", count, box)
                assert 10 < count < 3000 * scale * scale and box is not None, (count, box)
                assert 30 * scale <= box[0] < 100 * scale and 20 * scale <= box[1] < 100 * scale, box
                patched.move(270, 220)
                moved = patched.capture(protocol, tag + "-moved", cursor=True)
                _, box = changed(empty, moved)
                assert box is not None and box[0] >= 140 * scale and box[1] >= 90 * scale, box
                assert changed(empty, patched.capture(protocol, tag + "-no-ghost"))[0] == 0
                full = patched.capture(protocol, tag + "-whole", output="Virtual-1")
                assert changed(empty, full.crop((100 * scale, 100 * scale, 320 * scale, 280 * scale)))[0] == 0
                assert patched.run("tesseract /tmp/" + tag + "-excluded.png stdout 2>/dev/null").strip() == ""

    with subtest("the software recorder retains its requested cursor in decoded video"):
        patched.run("hyprctl eval 'hl.monitor({ output = \"Virtual-1\", mode = \"1280x720@60\", position = \"0x0\", scale = 1, transform = 0 })'")
        patched.wait_for(lambda: all(json.loads(patched.run("hyprctl -j monitors"))[0][key] == value
                                    for key, value in (("scale", 1), ("transform", 0))))
        patched.move(160, 150)
        empty = patched.capture("wlr", "recording-cursor-excluded")
        patched.run("systemd-run --user --unit=record --setenv=WAYLAND_DISPLAY=" + patched.wayland
                    + " --setenv=XDG_RUNTIME_DIR=/run/user/1000 wf-recorder --no-damage --no-dmabuf"
                    + " -g '100,100 220x180' -r 10 -c ffv1 -x bgr0 -f /tmp/cursor.mkv")
        fixed.wait_until_succeeds("test -e /tmp/cursor.mkv")
        time.sleep(1)
        patched.run("systemctl --user kill --signal=SIGINT record")
        patched.wait_for(lambda: patched.run("systemctl --user is-active record || true").strip() == "inactive")
        assert patched.run("systemctl --user show record -p Result --value").strip() == "success"
        fixed.succeed("ffmpeg -loglevel error -i /tmp/cursor.mkv -frames:v 1 /tmp/recorded-cursor.png")
        fixed.copy_from_machine("/tmp/cursor.mkv", "fixed")
        fixed.copy_from_machine("/tmp/recorded-cursor.png", "fixed")
        recorded = Image.open(fixed.out_dir / "fixed/recorded-cursor.png").convert("RGB")
        count, box = changed(empty, recorded)
        assert 10 < count < 3000 and box is not None, (count, box)
        assert 30 <= box[0] < 100 and 20 <= box[1] < 100, box

    with subtest("a mirrored display still shows the source software cursor"):
        # Keep the real DRM output for QMP screenshots; use a simulated source.
        patched.run("hyprctl output create headless CAPTURE-1")
        patched.run("hyprctl eval 'hl.monitor({ output = \"CAPTURE-1\", mode = \"1280x720@60\", position = \"0x0\", scale = 1 })'")
        patched.run("hyprctl eval 'hl.monitor({ output = \"Virtual-1\", mode = \"1280x720@60\", scale = 1, transform = 0, mirror = \"CAPTURE-1\" })'")
        def mirrored():
            monitors = json.loads(patched.run("hyprctl -j monitors all"))
            source = next(m for m in monitors if m["name"] == "CAPTURE-1")
            return any(m["name"] == "Virtual-1" and m["mirrorOf"] == str(source["id"]) for m in monitors)
        patched.wait_for(mirrored)
        for scale, transform, destination_transform in ((1, 0, 0), (2, 0, 0), (1, 1, 0), (1, 0, 1)):
            patched.run("hyprctl eval " + shlex.quote(
                f'hl.monitor({{ output = "CAPTURE-1", mode = "1280x720@60", position = "0x0", scale = {scale}, transform = {transform} }})'))
            patched.run("hyprctl eval " + shlex.quote(
                f'hl.monitor({{ output = "Virtual-1", mode = "1280x720@60", scale = 1, transform = {destination_transform}, mirror = "CAPTURE-1" }})'))
            def source_ready():
                monitors = json.loads(patched.run("hyprctl -j monitors all"))
                source = next(m for m in monitors if m["name"] == "CAPTURE-1")
                destination = next(m for m in monitors if m["name"] == "Virtual-1")
                return source["scale"] == scale and source["transform"] == transform and destination["transform"] == destination_transform
            patched.wait_for(source_ready)
            tag = f"mirror-scale{scale}-transform{transform}-destination{destination_transform}"
            patched.move(500, 300)
            fixed.screenshot(tag + "-away")
            patched.move(160, 150)
            fixed.screenshot(tag + "-pointer")
            width, height = (720, 1280) if transform else (1280, 720)
            target_width, target_height = (720, 1280) if destination_transform else (1280, 720)
            ratio = min(target_width / width, target_height / height)
            x = (target_width - width * ratio) / 2 + 160 * scale * ratio
            y = (target_height - height * ratio) / 2 + 150 * scale * ratio
            crop = (int(x - 40 * scale), int(y - 40 * scale), int(x + 60 * scale), int(y + 60 * scale))
            a = Image.open(fixed.out_dir / (tag + "-away.png")).convert("RGB")
            b = Image.open(fixed.out_dir / (tag + "-pointer.png")).convert("RGB")
            # QMP stores the physical scanout. Compare in logical orientation;
            # retain both unmodified original-resolution screenshots as evidence.
            if destination_transform:
                a, b = a.transpose(Image.Transpose.ROTATE_270), b.transpose(Image.Transpose.ROTATE_270)
            a, b = a.crop(crop), b.crop(crop)
            count, box = changed(a, b)
            print(tag, "mirror cursor pixels", count, box)
            assert 10 < count < 3000 * scale * scale, (count, box)

    with subtest("mirrored cursor fades to black with source DPMS and wakes unchanged"):
        patched.run("hyprctl eval 'hl.monitor({ output = \"Virtual-1\", mode = \"1280x720@60\", scale = 1, transform = 0, mirror = \"CAPTURE-1\" })'")
        patched.wait_for(lambda: any(m["name"] == "Virtual-1" and m["transform"] == 0
                                    for m in json.loads(patched.run("hyprctl -j monitors all"))))
        patched.move(160, 150)
        def pointer_image(name):
            fixed.screenshot(name)
            return Image.open(fixed.out_dir / (name + ".png")).convert("RGB").crop((140, 130, 200, 200))
        awake = pointer_image("dpms-awake")
        def peak(image):
            return max(sum(pixel) for pixel in image.getdata())
        patched.run("hyprctl eval 'hl.config({ animations = { enabled = true } }); hl.curve(\"linear\", { type = \"bezier\", points = { { 0, 0 }, { 1, 1 } } }); hl.animation({ leaf = \"fadeDpms\", enabled = true, speed = 10, bezier = \"linear\" })'")
        patched.run("hyprctl dispatch 'hl.dsp.dpms({ action = \"off\", monitor = \"CAPTURE-1\" })'")
        values = [peak(awake)]
        for i in range(30):
            values.append(peak(pointer_image(f"dpms-fade-{i}")))
            if values[-1] == 0:
                break
            time.sleep(0.1)
        print("DPMS fade cursor-region brightness", values)
        assert values[-1] == 0 and all(b <= a + 3 for a, b in zip(values, values[1:])), values
        monitors = json.loads(patched.run("hyprctl -j monitors all"))
        assert not next(m for m in monitors if m["name"] == "CAPTURE-1")["dpmsStatus"]
        assert next(m for m in monitors if m["name"] == "Virtual-1")["dpmsStatus"]
        patched.run("hyprctl dispatch 'hl.dsp.dpms({ action = \"on\", monitor = \"CAPTURE-1\" })'")
        for i in range(30):
            woke = pointer_image(f"dpms-wake-{i}")
            if changed(awake, woke)[0] == 0:
                break
            time.sleep(0.1)
        assert changed(awake, woke)[0] == 0, "cursor did not recover its color and position"
        patched.run("hyprctl eval 'hl.config({ animations = { enabled = false } })'")
        with subtest("a zoomed mirror retains the displayed cursor geometry"):
            patched.move(640, 360)
            patched.run("hyprctl eval 'hl.config({ cursor = { zoom_factor = 2 } })'")
            time.sleep(0.5)
            check_zoomed_cursor("mirror-zoom2", "CAPTURE-1")
            patched.run("hyprctl eval 'hl.config({ cursor = { zoom_factor = 1 } })'")
            time.sleep(0.2)

    with subtest("solid desktop background is preserved in the cursor-free image"):
        patched.run("hyprctl eval 'hl.monitor({ output = \"Virtual-1\", mode = \"1280x720@60\", position = \"0x0\", scale = 1, transform = 0, mirror = \"\" })'")
        patched.run("hyprctl output remove CAPTURE-1")
        patched.wait_for(lambda: any(m["name"] == "Virtual-1" and m["mirrorOf"] == "none"
                                    and m["scale"] == 1 and m["transform"] == 0
                                    for m in json.loads(patched.run("hyprctl -j monitors"))))
        with subtest("zoomed capture tracks ordinary, clamped and detached camera transforms"):
            for tag, rigid, detached, scale, transform, point in (("center", False, False, 1, 0, (640, 360)),
                                                ("clamped", True, False, 1, 0, (80, 60)),
                                                ("detached", False, True, 1, 0, (640, 360)),
                                                ("scale2", False, False, 2, 0, (320, 180)),
                                                ("rotated", False, False, 1, 1, (360, 640))):
                patched.run("hyprctl eval " + shlex.quote(
                    f'hl.monitor({{ output = "Virtual-1", mode = "1280x720@60", position = "0x0", scale = {scale}, transform = {transform} }})'))
                patched.wait_for(lambda: all(json.loads(patched.run("hyprctl -j monitors"))[0][key] == value
                                            for key, value in (("scale", scale), ("transform", transform))))
                patched.move(*point)
                patched.run("hyprctl eval " + shlex.quote(
                    f'hl.config({{ cursor = {{ zoom_factor = 2, zoom_rigid = {str(rigid).lower()}, zoom_detached_camera = {str(detached).lower()} }} }})'))
                time.sleep(0.5)
                check_zoomed_cursor("zoom2-" + tag, "Virtual-1", scale, transform)
                if detached:
                    patched.move(1000, 600)
                    check_zoomed_cursor("zoom2-detached-panned", "Virtual-1")
                patched.run("hyprctl eval 'hl.config({ cursor = { zoom_factor = 1, zoom_rigid = false, zoom_detached_camera = false } })'")
                time.sleep(0.2)
        patched.run("hyprctl eval 'hl.monitor({ output = \"Virtual-1\", mode = \"1280x720@60\", position = \"0x0\", scale = 1, transform = 0 })'")
        patched.wait_for(lambda: all(json.loads(patched.run("hyprctl -j monitors"))[0][key] == value
                                    for key, value in (("scale", 1), ("transform", 0))))
        patched.run("systemctl --user stop fixture")
        patched.wait_for(lambda: not json.loads(patched.run("hyprctl -j clients")))
        patched.move(500, 300)
        fixed.screenshot("solid-background-display")
        captured = patched.capture("ext", "solid-background-capture")
        displayed = Image.open(fixed.out_dir / "solid-background-display.png").convert("RGB").crop((100, 100, 320, 280))
        colors = displayed.getcolors(displayed.width * displayed.height)
        assert colors is not None and len(colors) == 1
        assert changed(displayed, captured)[0] == 0, (displayed.getpixel((0, 0)), captured.getpixel((0, 0)))

    for compositor in (baseline, patched):
        assert json.loads(compositor.run("hyprctl -j instances"))[0]["pid"] == compositor.pid
        compositor.machine.copy_from_machine("/tmp/hyprland.log", compositor.machine.name)
  '';
}
