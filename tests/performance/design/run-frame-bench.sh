#!/usr/bin/env bash
# The compositor's processor time per frame on this machine, with the
# compositor's own benchmark (desktop/aizome/crates/aizome-comp/examples/
# frame_bench.rs): GLES on each GPU and pixman (software), at the
# display's 3840 × 2160, scale 2, REPS rounds, alternating.
#
#   tests/performance/design/run-frame-bench.sh OUTDIR TARGET [REPS]
#
# TARGET is a cargo target directory holding a release build of
#   cargo build --release -p aizome-comp -p aizome-comp-logic --bin aizome --examples
# (in Aizome's development shell, which also runs it: the compositor finds
# libwayland-server and libEGL through its library path). LOCK (optional)
# wraps each round. Each run's report goes to OUTDIR/<renderer>-<round>.txt,
# the compositor's warnings and errors to OUTDIR/<renderer>-<round>.log; a
# run that fails is noted in rounds.log and the round goes on.
set -uo pipefail
out=$1 target=$2 reps=${3:-3}
repo=$(cd "$(dirname "$0")/../../.." && pwd)
mkdir -p "$out"
bench=$target/release/examples/frame_bench
mesa=/run/opengl-driver/share/glvnd/egl_vendor.d/50_mesa.json
nvidia=/run/opengl-driver/share/glvnd/egl_vendor.d/10_nvidia.json
for i in $(seq 1 "$reps"); do
  ${LOCK:-} bash -c "
    echo \"# round $i \$(date -Is) load \$(cut -d' ' -f1-3 /proc/loadavg)\"
    one() {
      local name=\$1; shift
      nix develop $repo#aizome -c env \"\$@\" $bench --renderer \${name%%-*} --output 3840x2160@2 --log warn > $out/\$name-$i.txt 2> $out/\$name-$i.log \
        || echo \"\$name round $i failed: exit \$?\"
    }
    one gles-radeon AIZOME_RENDER_NODE=/dev/dri/renderD128 __EGL_VENDOR_LIBRARY_FILENAMES=$mesa
    one gles-nvidia AIZOME_RENDER_NODE=/dev/dri/renderD129 __EGL_VENDOR_LIBRARY_FILENAMES=$nvidia
    one pixman
  " | tee -a "$out/rounds.log"
done
