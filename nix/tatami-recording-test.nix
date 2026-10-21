{
  pkgs,
  hyprland,
  tatami,
}:
let
  # Picker cancellation and recorder startup failure are explicit component
  # faults; successful recording uses real Hyprland, PipeWire and wf-recorder.
  picker = pkgs.writeShellScriptBin "slurp" ''
    test ! -e /tmp/cancel-capture || exit 1
    echo '0,0 1280x720'
  '';
  recorder = pkgs.writeShellScriptBin "wf-recorder" ''
    test ! -e /tmp/fail-recorder || exit 1
    exec ${pkgs.wf-recorder}/bin/wf-recorder "$@"
  '';
in
pkgs.testers.runNixOSTest {
  name = "tatami-recording-audio";
  globalTimeout = 480;
  nodes.machine = {
    imports = [ (pkgs.path + "/nixos/tests/common/user-account.nix") ];
    virtualisation = {
      memorySize = 3072;
      cores = 2;
      qemu.options = [ "-vga none -device virtio-gpu-pci" ];
    };
    hardware.graphics.enable = true;
    programs.hyprland = {
      enable = true;
      package = hyprland;
    };
    services.pipewire = {
      enable = true;
      pulse.enable = true;
    };
    security.rtkit.enable = true;
    environment.systemPackages = [
      tatami
      pkgs.pipewire
      pkgs.pulseaudio
      pkgs.ffmpeg
      pkgs.python3
      pkgs.gpu-screen-recorder
    ];
    environment.etc."hypr-test.lua".text = ''
      hl.monitor({ output = "", mode = "1280x720@60", position = "auto", scale = 1 })
    '';
    services.getty.autologinUser = "alice";
    programs.bash.loginShellInit = ''
      if [ "$(tty)" = /dev/tty1 ]; then
        export LIBGL_ALWAYS_SOFTWARE=1
        exec Hyprland -c /etc/hypr-test.lua > /tmp/hyprland.log 2>&1
      fi
    '';
  };
  testScript = ''
    import json, shlex, time
    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.wait_until_succeeds("test -S /run/user/1000/hypr/*/.socket.sock", timeout=45)
    signature = machine.succeed("basename /run/user/1000/hypr/*").strip()
    environment = "export XDG_RUNTIME_DIR=/run/user/1000 HYPRLAND_INSTANCE_SIGNATURE=" + signature + "; "
    def run(command):
        return machine.succeed("su - alice -c " + shlex.quote(environment + command))
    machine.wait_until_succeeds("su - alice -c " + shlex.quote(environment + "hyprctl -j instances | python3 -c 'import json,sys; assert json.load(sys.stdin)'"), timeout=30)
    wayland = json.loads(run("hyprctl -j instances"))[0]["wl_socket"]
    environment += "export WAYLAND_DISPLAY=" + wayland + "; "
    run("systemctl --user start pipewire-pulse.service wireplumber.service")
    machine.wait_until_succeeds("su - alice -c 'XDG_RUNTIME_DIR=/run/user/1000 pactl info'")
    run("pactl load-module module-null-sink sink_name=desktop_test")
    run("pactl load-module module-null-sink sink_name=microphone_test")
    microphone_module = run("pactl load-module module-remap-source master=microphone_test.monitor source_name=microphone_input").strip()
    run("pactl set-default-sink desktop_test; pactl set-default-source microphone_input")
    run("ffmpeg -v error -f lavfi -i sine=frequency=440:duration=360 -ar 48000 -ac 2 /tmp/desktop.wav")
    run("ffmpeg -v error -f lavfi -i sine=frequency=880:duration=360 -ar 48000 -ac 2 /tmp/microphone.wav")
    run("systemd-run --user --unit=tone-desktop paplay --device=desktop_test /tmp/desktop.wav")
    run("systemd-run --user --unit=tone-microphone paplay --device=microphone_test /tmp/microphone.wav")
    defaults = run("pactl get-default-sink; pactl get-default-source")
    def wait_for(command):
        try:
            machine.wait_until_succeeds("su - alice -c " + shlex.quote(environment + command), timeout=20)
        except Exception:
            print(run("journalctl --user -b --no-pager; ps -ef; for p in $(pgrep -x wf-recorder); do cat /proc/$p/status /proc/$p/wchan; done; pw-dump"))
            raise
    def start(args):
        run("rm -f ~/Videos/*.mp4")
        run("systemd-run --user --unit=record-test --collect --property=KillMode=process "
            "--setenv=XDG_RUNTIME_DIR=/run/user/1000 --setenv=HYPRLAND_INSTANCE_SIGNATURE=" + signature +
            " --setenv=WAYLAND_DISPLAY=" + wayland +
            " --setenv=PATH=${picker}/bin:${recorder}/bin:/run/current-system/sw/bin tatami screenrecord " + args)
    def clean():
        wait_for("! pgrep -x wf-recorder && ! pw-dump | grep -q tatami_recording_")
        assert defaults == run("pactl get-default-sink; pactl get-default-source")
    def stop():
        run("tatami screenrecord --stop")
        clean()
    def audio(expected):
        # Inspect decoded content, not just the presence of an audio stream.
        script = r"""
    import glob,json,math,struct,subprocess,wave
    path=glob.glob('/home/alice/Videos/*.mp4')[0]
    info=json.loads(subprocess.check_output(['ffprobe','-v','quiet','-show_streams','-of','json',path]))
    assert any(s['codec_type']=='video' and s['width']==1280 for s in info['streams'])
    if EXPECTED == []:
        assert not any(s['codec_type']=='audio' for s in info['streams'])
    else:
        subprocess.run(['ffmpeg','-v','error','-y','-i',path,'-vn','-ac','1','-ar','48000','/tmp/recorded.wav'],check=True)
        with wave.open('/tmp/recorded.wav') as w:
            raw=w.readframes(w.getnframes()); v=struct.unpack('<'+'h'*(len(raw)//2),raw)[48000:96000]
        assert len(v)==48000
        amplitudes={f:2*math.hypot(sum(x*math.cos(2*math.pi*f*i/48000) for i,x in enumerate(v)),sum(x*math.sin(2*math.pi*f*i/48000) for i,x in enumerate(v)))/len(v) for f in (440,880)}
        print(amplitudes)
        for frequency,value in amplitudes.items():
            assert value>500 if frequency in EXPECTED else value<20, amplitudes
    """.replace('EXPECTED',repr(expected))
        print(machine.succeed("python3 -c " + shlex.quote(script)))

    with subtest("both independent tones survive real software recording and stop"):
        start("--desktop-audio --microphone")
        wait_for("pgrep -a -x wf-recorder | grep -- --audio=tatami_recording_")
        time.sleep(3)
        stop()
        audio([440,880])
    with subtest("existing no-audio, desktop and microphone paths retain their content"):
        for args,expected in [("",[]),("--desktop-audio",[440]),("--microphone",[880])]:
            start(args)
            wait_for("pgrep -x wf-recorder")
            time.sleep(3)
            stop()
            audio(expected)
    with subtest("cancel and failed recorder leave no mixer"):
        machine.succeed("touch /tmp/cancel-capture")
        start("--desktop-audio --microphone")
        wait_for("! systemctl --user is-active --quiet record-test")
        clean()
        machine.succeed("rm /tmp/cancel-capture; touch /tmp/fail-recorder")
        start("--desktop-audio --microphone")
        wait_for("! systemctl --user is-active --quiet record-test")
        clean()
        machine.succeed("rm /tmp/fail-recorder")
    with subtest("killing the supervisor removes recorder and temporary graph"):
        start("--desktop-audio --microphone")
        wait_for("pgrep -x wf-recorder")
        # Main only: child cleanup must come from parent-death hooks, not
        # systemd killing the whole cgroup for this test.
        run("systemctl --user kill --kill-whom=main --signal=KILL record-test")
        clean()
        run("systemctl --user reset-failed")
    with subtest("an unavailable selected microphone refuses mixed recording"):
        run("pactl unload-module " + microphone_module)
        start("--desktop-audio --microphone")
        wait_for("! systemctl --user is-active --quiet record-test")
        wait_for("! pgrep -x wf-recorder && ! pw-dump | grep -q tatami_recording_")
        microphone_module = run("pactl load-module module-remap-source master=microphone_test.monitor source_name=microphone_input").strip()
        run("pactl set-default-source microphone_input")
        clean()
    with subtest("a killed mixer stops the recorder, and a fresh run recovers"):
        start("--desktop-audio --microphone")
        wait_for("pgrep -x wf-recorder")
        time.sleep(3)
        run("pkill -KILL -x pw-cli")
        clean()
        audio([440,880])
    with subtest("a disappearing microphone stops and finalizes the recording"):
        start("--desktop-audio --microphone")
        wait_for("pgrep -x wf-recorder")
        time.sleep(3)
        run("pactl unload-module " + microphone_module)
        wait_for("! pgrep -x wf-recorder && ! pw-dump | grep -q tatami_recording_")
        audio([440,880])
        microphone_module = run("pactl load-module module-remap-source master=microphone_test.monitor source_name=microphone_input").strip()
        run("pactl set-default-source microphone_input")
        clean()
    with subtest("fresh recording recovers after source loss"):
        start("--desktop-audio --microphone")
        wait_for("pgrep -x wf-recorder")
        time.sleep(3)
        stop()
        audio([440,880])
    machine.shutdown()
  '';
}
