#!/usr/bin/env bash
set -euo pipefail
base=/home/callen/work/hotaru-region-package-20261028
mirror=/home/callen/work/sekka-hotaru-nix-20261028
exec 9>/home/callen/.cache/hotaru-workstation.lock
flock -x 9
/nix/var/nix/profiles/default/bin/nix build "path:$mirror#hotaru" --no-update-lock-file --out-link "$base/result" --cores 32 --max-jobs 2 -L >"$base/nix-build.log" 2>&1
python3 "$base/nix-checkpoint.py" --root "$base" --scope '434 sprites and bounded region pixel reuse; separate native runtime qualification'
prepared=$(python3 -c 'import json;print(json.load(open("/home/callen/work/hotaru-region-package-20261028/package-checkpoint.json"))["source"])')
/nix/var/nix/profiles/default/bin/nix-store --add-root "$base/prepared-source" --indirect --realise "$prepared"
bash "$base/qualify.sh" "$mirror" "$base/result" "$prepared" "$base/native" /home/callen/work/hotaru-sprites-braille-20261028/oracle-expanded/libsprites-fast.so >"$base/qualification.log" 2>&1
