# Packaged Shashin verifies the shared panel returns exactly the selected files.
{
  pkgs,
  self,
  package ? null,
}:
let
  pictures =
    pkgs.runCommand "file-panel-pictures"
      {
        nativeBuildInputs = [ (pkgs.python3.withPackages (ps: [ ps.pillow ])) ];
      }
      ''
        mkdir -p "$out"
        python - "$out" <<'PY'
        import sys
        from pathlib import Path
        from PIL import Image
        for name, color in [("first.png", "#f4a261"), ("second.png", "#82c4a8"), ("third.png", "#243b55")]:
            Image.new("RGB", (320, 200), color).save(Path(sys.argv[1]) / name)
        PY
      '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-file-panel-multiple";
  globalTimeout = 300;
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
    environment.etc."panel-pictures".source = pictures;
  };
  testScript = builtins.readFile ./vm-file-panel-multiple-test.py;
}
