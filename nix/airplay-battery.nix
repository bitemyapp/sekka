{
  lib,
  writeText,
  writeTextFile,
  python3,
  iproute2,
  iputils,
  systemd,
  callPackage,
  airplay,
  airplay-radio,
  airplay-peer-session,
  airplay-intel-driver,
  doubletake,
}:
let
  helper = options: callPackage ./airplay-peer-session.nix ({ inherit airplay-radio; } // options);
  binary = package: "${package}/bin/sekka-airplay-peer-session";
  driver = airplay-intel-driver.override { trace = true; };
  deadlineSender = doubletake.overrideAttrs (old: {
    pname = "doubletake-keyframe-grace-trial";
    postPatch = old.postPatch + ''
      substituteInPlace internal/airplay/mirror.go --replace-fail \
        's.dataConn.SetWriteDeadline(time.Now().Add(2 * time.Second))' \
        's.dataConn.SetWriteDeadline(time.Now().Add(8 * time.Second))'
    '';
  });
  bufferedSender = doubletake.overrideAttrs (old: {
    pname = "doubletake-send-buffer-trial";
    postPatch = old.postPatch + ''
      substituteInPlace internal/airplay/mirror.go --replace-fail \
        'tc.SetWriteBuffer(64 * 1024)' 'tc.SetWriteBuffer(256 * 1024)'
    '';
  });
  manifest = writeText "airplay-battery-manifest.json" (
    builtins.toJSON {
      version = 1;
      app = "${airplay}/bin/sekka-airplay";
      senders = {
        default = "${doubletake}/bin/doubletake";
        keyframe-grace = "${deadlineSender}/bin/doubletake";
        send-buffer = "${bufferedSender}/bin/doubletake";
      };
      cases = builtins.fromJSON (builtins.readFile ../tests/airplay-battery-cases.json);
      helpers = {
        stock = binary airplay-peer-session;
        p2p-go = binary (helper {
          airplay-radio = airplay-radio.override { p2pGo = true; };
        });
        qos-noack = binary (helper {
          airplay-radio = airplay-radio.override { qosNoAck = true; };
        });
        paced1 = binary (helper {
          airplay-radio = airplay-radio.override {
            pacedInjection = true;
            pacingUs = 1000;
          };
        });
        paced2 = binary (helper {
          airplay-radio = airplay-radio.override { pacedInjection = true; };
        });
        legacy-clock = binary (helper {
          airplay-radio = airplay-radio.override { peerClock = false; };
        });
        patched = binary (helper {
          intelDriver = driver;
        });
        performance = binary (helper {
          intelDriver = driver;
          intelPowerScheme = 1;
        });
        filter = binary (helper {
          intelDriver = driver.override { ibssMonitor = true; };
        });
      };
      tools = {
        pkexec = "/run/wrappers/bin/pkexec";
        nmcli = "/run/current-system/sw/bin/nmcli";
        ss = "${iproute2}/bin/ss";
        ping = "${iputils}/bin/ping";
        journalctl = "${systemd}/bin/journalctl";
        systemctl = "${systemd}/bin/systemctl";
      };
    }
  );
in
writeTextFile {
  name = "sekka-airplay-battery";
  destination = "/bin/sekka-airplay-battery";
  executable = true;
  text =
    lib.replaceStrings
      [ "#!/usr/bin/env python3" "@manifest@" ]
      [ "#!${python3}/bin/python3 -I" (toString manifest) ]
      (builtins.readFile ../scripts/airplay_battery.py);
  passthru = { inherit manifest; };
  meta = {
    description = "Opt-in AirPlay hardware test matrix with bounded radio leases";
    platforms = lib.platforms.linux;
    mainProgram = "sekka-airplay-battery";
    license = lib.licenses.mit;
  };
}
