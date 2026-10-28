# Packaged Utsushi in a real Aizome session: a video opened as the desktop
# opens one (xdg-open, the session's default for video/mp4), played to its
# end through PipeWire, and a song paused and resumed by its key. The VM's
# GPU has no 3D, so the compositor renders in software and takes no
# dmabufs: frames take Utsushi's shared-memory path, and decoding is in
# software. The hardware path (VA-API, dmabufs, planes) needs a real GPU.
{
  pkgs,
  self,
  package ? null,
}:
let
  media = pkgs.runCommand "utsushi-media" { nativeBuildInputs = [ pkgs.ffmpeg ]; } ''
    mkdir -p "$out"
    ffmpeg -nostdin -loglevel error -f lavfi -i "testsrc2=size=320x180:rate=25:duration=6" \
      -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=6" -ac 2 \
      -c:v libx264 -preset veryfast -g 25 -pix_fmt yuv420p -c:a aac -b:a 64k \
      -metadata title="Test Pattern" -movflags +faststart "$out/pattern.mp4"
    ffmpeg -nostdin -loglevel error -f lavfi -i "sine=frequency=523:sample_rate=44100:duration=20" \
      -c:a flac -metadata title="Long Note" -metadata artist="The Dyers" "$out/note.flac"
  '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-utsushi";
  globalTimeout = 900;
  nodes.machine =
    { lib, ... }:
    {
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
      # No sound card: a sink that takes the stream at the device's pace.
      services.pipewire.extraConfig.pipewire."91-test-sink"."context.objects" = [
        {
          factory = "adapter";
          args = {
            "factory.name" = "support.null-audio-sink";
            "node.name" = "test-sink";
            "media.class" = "Audio/Sink";
            "audio.position" = "FL,FR";
            "object.linger" = true;
            "priority.session" = 1009;
            "priority.driver" = 1009;
          };
        }
      ];
      environment.systemPackages = [
        pkgs.xdg-utils
        pkgs.pipewire
      ];
      environment.etc."utsushi-media".source = media;
    };
  testScript = builtins.readFile ./vm-utsushi-test.py;
}
