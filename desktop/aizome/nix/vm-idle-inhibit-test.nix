# A video playing holds off the idle lock and the displays' turning off, in
# a real Aizome session with a minute's idleness set for both: Utsushi
# (Aizome's player), mpv and VLC as Nixpkgs packages them, each started as
# its desktop entry starts it, and a page's video in Chromium: as it starts
# here, when the video starts by itself with the person in another window,
# and as an X11 client. Paused, each lets the session go idle again, and so
# does VLC killed while it plays (vm-idle-inhibit-test.py). While each
# plays, the cup in the menu bar shows for it and its panel names it; and
# locked by hand with mpv or VLC playing, the session turns its displays
# off on their minute all the same.
#
# The VM's GPU has no 3D, so the players decode and draw in software. What
# keeps the session awake doesn't depend on that: a Wayland idle inhibitor
# (Utsushi, mpv, Chromium for a video started in its own window), or the
# session bus's org.freedesktop.ScreenSaver, which the shell serves (VLC,
# and Chromium otherwise).
#
# Chromium stands for Google Chrome, which did the same in each of these
# when both were tried by hand. Chrome isn't free software, and this check
# needs nothing that isn't.
#
#     nix build .#aizome-checks.idle-inhibit -L
{
  pkgs,
  self,
  package ? null,
}:
let
  # A page with a video and nothing else. Its keys are a person's: p plays
  # and pauses, f goes full screen and back, and d plays twenty seconds
  # later, by which time the person is in another window (the test checks
  # that they were), as when a playlist's next video starts. Its title says
  # what the video is doing, which the test reads from the window.
  page = pkgs.writeText "video.html" ''
    <!doctype html>
    <meta charset="utf-8">
    <title>loading</title>
    <style>html,body{margin:0;background:#000;height:100%}video{width:100%;height:100%}</style>
    <video id="v" src="pattern.webm" loop></video>
    <script>
    const v = document.getElementById("v");
    addEventListener("keydown", e => {
      if (e.key === "p") { v.paused ? v.play() : v.pause(); }
      if (e.key === "f") { document.fullscreenElement ? document.exitFullscreen() : v.requestFullscreen(); }
      if (e.key === "d") { setTimeout(() => v.play(), 20000); }
    });
    setInterval(() => {
      document.title = (v.paused ? "paused" : "playing") + " " + Math.floor(v.currentTime * 10)
        + (document.fullscreenElement ? " fullscreen" : " windowed");
    }, 250);
    </script>
  '';
  # Generated here, a test card and a tone: 20 seconds, looped by each
  # player. The page's is WebM, which every Chromium plays.
  media = pkgs.runCommand "idle-inhibit-media" { nativeBuildInputs = [ pkgs.ffmpeg ]; } ''
    mkdir -p "$out"
    ffmpeg -nostdin -loglevel error -f lavfi -i "testsrc2=size=320x180:rate=25:duration=20" \
      -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=20" -ac 2 \
      -c:v libx264 -preset veryfast -g 25 -pix_fmt yuv420p -c:a aac -b:a 64k \
      -metadata title="Test Pattern" -movflags +faststart "$out/pattern.mp4"
    ffmpeg -nostdin -loglevel error -f lavfi -i "testsrc2=size=320x180:rate=25:duration=20" \
      -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=20" -ac 2 \
      -c:v libvpx -b:v 300k -g 25 -c:a libvorbis "$out/pattern.webm"
    cp ${page} "$out/video.html"
  '';
in
pkgs.testers.runNixOSTest {
  name = "aizome-idle-inhibit";
  # It takes about twenty minutes on a host with other work to do.
  globalTimeout = 2400;
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
        pkgs.chromium
        # xdg-open, as the desktop opens a video.
        pkgs.xdg-utils
      ];
      environment.etc."idle-inhibit-media".source = media;
    };
  testScript = builtins.readFile ./vm-idle-inhibit-test.py;
}
