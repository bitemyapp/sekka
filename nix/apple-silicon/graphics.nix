# The M3 Pro's session graphics around mesa.nix: Omarchy's runtime GPU ABI
# check, its driconf mapping, the uwsm session gate (m3-gpu/session-env.sh),
# the check that a login on the GPU came up, and the GPU performance profile
# its release pairs with this Mesa (tools/aurora-sep/pro, aurora-2026.10.17.20).
{
  lib,
  stdenv,
  pkg-config,
  libdrm,
  writeText,
  writeShellScript,
  runCommand,
  replaceVars,
  bash,
  coreutils,
  gnugrep,
  gnused,
  mesa,
}:
let
  # mesa-m3-abi-check.c and mesa-m3-profile.h are Omarchy's (MIT, Chris
  # Kearney), against the native Mesa tree's asahi_drm.h.
  abiCheck = stdenv.mkDerivation {
    pname = "mesa-m3-abi-check";
    version = mesa.native.version;
    src = ./m3-gpu;
    nativeBuildInputs = [ pkg-config ];
    buildInputs = [ libdrm ];
    buildPhase = ''
      $CC -std=gnu11 -O2 -Wall -Werror -I${mesa.native.src}/include \
        -o mesa-m3-abi-check mesa-m3-abi-check.c $(pkg-config --cflags --libs libdrm)
    '';
    installPhase = "install -Dm755 mesa-m3-abi-check $out/bin/mesa-m3-abi-check";
    meta.mainProgram = "mesa-m3-abi-check";
  };

  # Which DRI driver each kernel driver gets (10-m3-pro.conf), with Mesa's
  # defaults beside it, since DRIRC_CONFIGDIR replaces the default directory.
  drirc = runCommand "m3-pro-drirc" { } ''
    mkdir -p $out
    cp ${mesa.native}/share/drirc.d/*.conf $out/
    cp ${writeText "10-m3-pro.conf" ''
      <driconf>
       <device driver="loader" kernel_driver="asahi"><application name="all"><option name="dri_driver" value="asahi" /></application></device>
       <device driver="loader" kernel_driver="m3-dcp"><application name="all"><option name="dri_driver" value="asahi" /></application></device>
       <device driver="loader" kernel_driver="apple"><application name="all"><option name="dri_driver" value="apple" /></application></device>
       <device driver="loader" kernel_driver="softpipe"><application name="all"><option name="dri_driver" value="kms_swrast" /></application></device>
       <device driver="loader" kernel_driver="simpledrm"><application name="all"><option name="dri_driver" value="kms_swrast" /></application></device>
      </driconf>
    ''} $out/10-m3-pro.conf
  '';

  sessionEnv = replaceVars ./m3-gpu/session-env.sh {
    abiCheck = lib.getExe abiCheck;
    native = mesa.native;
    vulkan = mesa.vulkan;
    inherit drirc;
  };

  # Run by the session as sekka-m3-gpu-confirm.service once the graphical
  # session is up: if it is still up after the settle time, this login on the
  # GPU counts as working, and the next login may use the GPU again.
  confirm = writeShellScript "sekka-m3-gpu-confirm" ''
    set -eu
    ${coreutils}/bin/sleep 15
    if [ -z "''${WAYLAND_DISPLAY:-}" ] || [ ! -S "$XDG_RUNTIME_DIR/$WAYLAND_DISPLAY" ]; then
      echo "no Wayland compositor is answering; this login on the GPU stays unconfirmed"
      exit 1
    fi
    dir=''${XDG_STATE_HOME:-$HOME/.local/state}/sekka-m3-gpu
    ${coreutils}/bin/mkdir -p "''${dir%/*}"
    [ -d "$dir" ] || ${coreutils}/bin/mkdir -m 0700 "$dir"
    printf 'boot_id=%s\nrun=%s\n' "$(${coreutils}/bin/cat /proc/sys/kernel/random/boot_id)" \
      "$SEKKA_M3_GPU_RUN" > "$dir/session-ok.new"
    ${coreutils}/bin/mv -f "$dir/session-ok.new" "$dir/session-ok"
  '';

  # The batching controls Omarchy's release tested with this Mesa
  # (mesa-m3-pro-performance), set once the GPU passes the same identity check.
  performance = writeShellScript "sekka-m3-gpu-performance" ''
    set -euo pipefail
    case " $(cat /proc/cmdline) " in *" mesa_m3=off "*) exit 0 ;; esac
    [[ -e /etc/mesa-m3/disable ]] && exit 0
    for _ in {1..300}; do
      [[ -r /sys/kernel/debug/asahi-m3/completion_wait ]] && break
      sleep 0.1
    done
    node=
    for n in /sys/class/drm/renderD*; do
      [[ $(basename "$(readlink -f "$n/device/driver")") == asahi ]] && node=/dev/dri/''${n##*/}
    done
    if [[ -z $node ]]; then
      echo "no asahi render node; the profile stays at the driver's defaults"
      exit 0
    fi
    ${lib.getExe abiCheck} legacy "$node"
    [[ $(cat /sys/module/asahi/parameters/m3_consistent_perf) == 0 ]]
    [[ $(cat /sys/module/asahi/parameters/m3_pipeline_depth) == 0 ]]
    printf 4 > /sys/module/asahi/parameters/m3_render_batch_override
    printf 4 > /sys/module/asahi/parameters/m3_compute_batch_override
    printf 25 > /sys/kernel/debug/asahi-m3/completion_wait
    printf 1 > /sys/kernel/debug/asahi-m3/submit_overlap
    printf 1 > /sys/module/asahi/parameters/m3_mixed_batch
    echo 'T6030 performance profile: render/compute batches 4, completion wait 25 us, mixed batches and preparation overlap'
  '';
  # The gate's decisions against fake T6030 roots (m3-gpu/test-session-env.sh).
  sessionEnvTest =
    runCommand "sekka-m3-gpu-session-env-test"
      {
        nativeBuildInputs = [
          bash
          coreutils
          gnugrep
          gnused
        ];
      }
      ''
        bash ${./m3-gpu/test-session-env.sh} ${sessionEnv}
        touch $out
      '';
in
{
  inherit
    abiCheck
    drirc
    sessionEnv
    sessionEnvTest
    confirm
    performance
    ;
}
