#!/usr/bin/env bash
# Run only after the root agent confirms the final package and source freeze.
# Usage: bash qualify.sh MIRROR PACKAGE PREPARED_SOURCE EVIDENCE_DIRECTORY SPRITE_ORACLE
# The caller holds /home/callen/.cache/hotaru-workstation.lock for this script.
set -euo pipefail
script_dir=$(dirname "$(realpath "$0")")
mirror=$(realpath "$1")
package=$(realpath "$2")
prepared=$(realpath "$3")
evidence=$(realpath -m "$4")
oracle=$(realpath "$5")
test -f "$oracle"
test -x "$package/bin/hotaru"
test -f "$prepared/vendor/sidestep/crates/sidestep-ui/src/app.rs"
test -L "$mirror/vendor/sidestep"
ln -sfn "$prepared/vendor/sidestep" "$mirror/vendor/sidestep"
mkdir -p "$evidence"
cd "$mirror"
for suite in smoke fonts search shell links cursor pixel_scroll; do
  mkdir -p "$evidence/$suite"
  env -u AIZOME_FONT_DIR -u HOTARU_FONT_DIR -u HOTARU_RESOURCES_DIR -u HOTARU_TERMINFO_DIR \
      -u FONTCONFIG_FILE -u FONTCONFIG_PATH -u LD_LIBRARY_PATH -u SIDESTEP_FONT -u SIDESTEP_MONO_FONT \
      PATH=/usr/bin:/bin python3 "desktop/aizome/apps/hotaru/tests/wayland/$suite.py" \
      "$package/bin/hotaru" "$evidence/$suite" >"$evidence/$suite/runner.log" 2>&1
  printf '%s passed\n' "$suite"
done
mkdir -p "$evidence/sprites"
env -u AIZOME_FONT_DIR -u HOTARU_FONT_DIR -u HOTARU_RESOURCES_DIR -u HOTARU_TERMINFO_DIR \
    -u FONTCONFIG_FILE -u FONTCONFIG_PATH -u LD_LIBRARY_PATH -u SIDESTEP_FONT -u SIDESTEP_MONO_FONT \
    PATH=/usr/bin:/bin python3 desktop/aizome/apps/hotaru/tests/wayland/sprites.py \
    "$package/bin/hotaru" "$oracle" "$evidence/sprites" >"$evidence/sprites/runner.log" 2>&1
printf '%s passed\n' sprites
python3 "$script_dir/provenance.py" "$mirror" "$package" "$prepared" "$evidence"
