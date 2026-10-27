# Component regression: Settei choices, session defaults and desktop pixels.
{
  pkgs,
  self,
  package ? null,
}:
let
  pictures =
    pkgs.runCommand "wallpaper-choice-pictures"
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
        Image.new("RGB", (160, 100), (192, 40, 40)).save(out / "night.png")
        Image.new("RGB", (160, 100), (40, 192, 80)).save(out / "day.png")
        PY
      '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-wallpaper-choice";
  globalTimeout = 300;
  extraPythonPackages = ps: [ ps.pillow ];
  nodes.machine = { lib, ... }: {
    imports = [ self.nixosModules.default ];
    programs.aizome = {
      enable = true;
      package = lib.mkIf (package != null) package;
      wallpapers = {
        night = "${pictures}/night.png";
        day = "${pictures}/day.png";
      };
    };
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
  };
  testScript = builtins.readFile ./vm-wallpaper-choice-test.py;
}
