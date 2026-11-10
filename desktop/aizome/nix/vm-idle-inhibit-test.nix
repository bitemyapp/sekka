# A video playing holds off the idle lock and the displays' turning off, in
# a real Aizome session with a minute's idleness set for both: Utsushi
# (Aizome's player), mpv and VLC as Nixpkgs packages them, each started as
# its desktop entry starts it. Paused, each lets the session go idle again,
# and so does VLC killed while it plays (vm-idle-inhibit-test.py).
#
# The VM's GPU has no 3D, so the players decode and draw in software. What
# keeps the session awake doesn't depend on that: a Wayland idle inhibitor
# (Utsushi, mpv), or the session bus's org.freedesktop.ScreenSaver, which
# the shell serves (VLC, an X11 client here).
#
#     nix build .#aizome-checks.idle-inhibit -L
{
  pkgs,
  self,
  package ? null,
}:
let
  # Generated here, a test card and a tone: 20 seconds, looped by each
  # player.
  media = pkgs.runCommand "idle-inhibit-media" { nativeBuildInputs = [ pkgs.ffmpeg ]; } ''
    mkdir -p "$out"
    ffmpeg -nostdin -loglevel error -f lavfi -i "testsrc2=size=320x180:rate=25:duration=20" \
      -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=20" -ac 2 \
      -c:v libx264 -preset veryfast -g 25 -pix_fmt yuv420p -c:a aac -b:a 64k \
      -metadata title="Test Pattern" -movflags +faststart "$out/pattern.mp4"
  '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-idle-inhibit";
  globalTimeout = 1800;
  nodes.machine =
    { lib, ... }:
    {
      imports = [ self.nixosModules.default ];
      programs.aizome.enable = true;
      programs.aizome.package = lib.mkIf (package != null) package;
      users.users.alice = {
        isNormalUser = true;
        uid = 1000;
        password = "aizome";
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
      # The shortest the settings go: a minute to the lock, and a minute
      # to the displays' turning off (which asks for the password too).
      systemd.tmpfiles.settings."10-idle-inhibit-test" = {
        "/home/alice/.config".d = {
          user = "alice";
          group = "users";
          mode = "0755";
        };
        "/home/alice/.config/aizome".d = {
          user = "alice";
          group = "users";
          mode = "0755";
        };
        "/home/alice/.config/aizome/settings.toml".f = {
          user = "alice";
          group = "users";
          mode = "0644";
          argument = "[idle]\nlock_after_minutes = 1\nmonitors_off_after_minutes = 1\n";
        };
      };
      environment.systemPackages = [
        pkgs.mpv
        pkgs.vlc
        # xdg-open, as the desktop opens a video.
        pkgs.xdg-utils
      ];
      environment.etc."idle-inhibit-media".source = media;
    };
  testScript = builtins.readFile ./vm-idle-inhibit-test.py;
}
