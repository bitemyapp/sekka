#!/run/current-system/sw/bin/bash
set -euo pipefail
base=/home/callen/work/hotaru-local-gui-20261028
source "$base/environment.sh"
exec /nix/store/3hqwbwxij9lxhyrf2amsm2xg24xszmmx-bubblewrap-0.12.0/bin/bwrap \
  --unshare-user --die-with-parent --new-session \
  --ro-bind / / --ro-bind "$base/nix-root/nix/store" /nix/store \
  --bind "$base" "$base" --bind /tmp /tmp \
  --bind /home/callen/.cache/hotaru-local-benchmark.lock /home/callen/.cache/hotaru-local-benchmark.lock \
  --proc /proc --dev /dev --dir /dev/dri --dev-bind /dev/dri/renderD128 /dev/dri/renderD128 -- "$@"
