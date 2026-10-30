# Component regression for the default player on a GPU without 3D support.
# This instrumented VM does not qualify an ISO or a normal GUI installation.
{ pkgs, inputs }:
let
  video = pkgs.runCommand "mpv-moving-video.mp4" { nativeBuildInputs = [ pkgs.ffmpeg ]; } ''
    ffmpeg -v error -f lavfi -i testsrc2=size=320x240:rate=30 \
      -t 4 -an -c:v libx264 -pix_fmt yuv420p -f mp4 "$out"
  '';
  options = pkgs.writeText "mpv-options.lua" ''
    print("SEKKA_OPTIONS=" .. mp.get_property("options/gpu-api") .. ":" .. mp.get_property("options/gpu-sw"))
    mp.commandv("quit", 0)
  '';
in
pkgs.testers.runNixOSTest {
  name = "sekka-mpv-software";
  globalTimeout = 600;
  node.pkgsReadOnly = false;
  nodes.machine = { ... }: {
    imports = [
      inputs.aizome.nixosModules.default
      inputs.tatami.nixosModules.default
    ];
    programs.aizome.enable = true;
    programs.tatami.enable = true;
    users.users.alice = {
      isNormalUser = true;
      uid = 1000;
      password = "test-only";
      extraGroups = [ "video" ];
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
      cores = 2;
      qemu.options = [
        "-vga none"
        "-device virtio-gpu-pci,xres=1280,yres=800"
      ];
    };
  };
  testScript = ''
    import shlex

    def alice(command):
        return "su - alice -c " + shlex.quote(
            "export XDG_RUNTIME_DIR=/run/user/1000; "
            "export WAYLAND_DISPLAY=$(ls /run/user/1000 | grep -m1 '^wayland-[0-9]*$'); "
            + command
        )

    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.wait_until_succeeds("pgrep -u alice -x aizome-shell", timeout=180)
    # Root can't look into the session's FUSE mounts (the document portal's
    # doc, gvfs). find says so and fails, which under the test driver's
    # pipefail failed every try once a slow start mounted them first.
    machine.wait_until_succeeds(
        "test -n \"$(find /run/user/1000 -maxdepth 1 -type s -name 'wayland-[0-9]*' 2>/dev/null)\"", timeout=180
    )

    with subtest("the shipped defaults play moving video through software OpenGL"):
        # Use the actual installed mpv and /etc config, with neither rendering
        # option supplied by the test. Finite playback must finish, not merely
        # open a window or decode one frame and stall. Three seconds of video
        # takes about four here, and a stall never finishes: the limit leaves
        # room for a busy host.
        machine.succeed(alice(
            "timeout 60s mpv --length=3 --log-file=/tmp/mpv-playback.log ${video}"
        ))
        playback_log = machine.succeed("cat /tmp/mpv-playback.log")
        assert "GL_RENDERER='llvmpipe" in playback_log, playback_log
        assert "VO: [gpu-next]" in playback_log, playback_log
        assert "Exiting... (End of file)" in playback_log, playback_log
        machine.copy_from_machine("/tmp/mpv-playback.log")
        machine.succeed("test -z \"$(coredumpctl --no-pager --no-legend list mpv 2>/dev/null)\"")

    with subtest("user and command-line preferences override system defaults"):
        inspect = "mpv --idle=yes --script=${options}"
        assert "SEKKA_OPTIONS=opengl,vulkan:yes" in machine.succeed(alice(inspect))
        machine.succeed(alice("mkdir -p ~/.config/mpv"))
        machine.succeed(alice("printf 'gpu-api=vulkan\\ngpu-sw=no\\n' > ~/.config/mpv/mpv.conf"))
        assert "SEKKA_OPTIONS=vulkan:no" in machine.succeed(alice(inspect))
        assert "SEKKA_OPTIONS=opengl:yes" in machine.succeed(alice(inspect + " --gpu-api=opengl --gpu-sw=yes"))
  '';
}
