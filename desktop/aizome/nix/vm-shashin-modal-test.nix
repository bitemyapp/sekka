# Real packaged Shashin: an unsaved-close alert owns keyboard/menu decisions.
{ pkgs, self }:
let
  picture =
    pkgs.runCommand "shashin-modal-picture.jpg"
      {
        nativeBuildInputs = [ (pkgs.python3.withPackages (ps: [ ps.pillow ])) ];
      }
      ''
        python - "$out" <<'PY'
        import sys
        from PIL import Image, ImageDraw
        image = Image.new("RGB", (640, 400), "#243b55")
        draw = ImageDraw.Draw(image)
        draw.rectangle((0, 0, 180, 150), fill="#f4a261")
        draw.ellipse((400, 200, 600, 380), fill="#82c4a8")
        image.save(sys.argv[1], "JPEG")
        PY
      '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-shashin-modal";
  globalTimeout = 300;
  nodes.machine = {
    imports = [ self.nixosModules.default ];
    programs.aizome.enable = true;
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
    environment.etc."shashin-modal.jpg".source = picture;
  };
  testScript = builtins.readFile ./vm-shashin-modal-test.py;
}
