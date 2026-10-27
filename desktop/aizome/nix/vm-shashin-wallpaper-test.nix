# Real packaged Shashin and desktop renderer, driven through the Edit menu.
{
  pkgs,
  self,
  package ? null,
}:
let
  pictures =
    pkgs.runCommand "shashin-wallpaper-pictures"
      {
        nativeBuildInputs = [ (pkgs.python3.withPackages (ps: [ ps.pillow ])) ];
      }
      ''
        mkdir -p "$out"
        python - "$out" <<'PY'
        import sys
        from pathlib import Path
        from PIL import Image
        out = Path(sys.argv[1])
        Image.new("RGB", (160, 100), (192, 40, 40)).save(out / "base.png")
        for ext, color in [("png", (40, 80, 192)), ("jpg", (192, 80, 40)),
                           ("webp", (80, 40, 192)), ("bmp", (40, 192, 160)),
                           ("tiff", (192, 160, 40)), ("ico", (160, 40, 192))]:
            im = Image.new("RGB", (256, 256), color)
            # PNG payloads in ICO require 32-bit RGBA. Pillow otherwise
            # emits RGB PNG entries that the image crate rejects.
            if ext == "ico":
                im = im.convert("RGBA")
            im.save(out / ("picture." + ext))
        Image.new("RGB", (160, 100), (40, 192, 80)).save(
            out / "picture.gif", save_all=True,
            append_images=[Image.new("RGB", (160, 100), (192, 40, 80))],
            duration=200, loop=0,
        )
        (out / "picture.svg").write_text('<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100"><rect width="160" height="100" fill="#28c050"/></svg>')
        # A misleading extension must not reject otherwise valid raster data.
        (out / "raster.svg").write_bytes((out / "picture.png").read_bytes())
        PY
      '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-shashin-wallpaper";
  globalTimeout = 600;
  extraPythonPackages = ps: [ ps.pillow ];
  nodes.machine = { lib, ... }: {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
    programs.aizome.package = lib.mkIf (package != null) package;
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
    environment.systemPackages = [ pkgs.python3 ];
    environment.etc."wallpaper-pictures".source = pictures;
  };
  testScript = builtins.readFile ./vm-shashin-wallpaper-test.py;
}
