#!/usr/bin/env bash
# The configuration alone does not reproduce a randomized kernel's layout
# seed, and this separately built module has no kernel signing key.
set -euo pipefail
config=$1
if ! grep -qx 'CONFIG_DRM_XE=m' "$config"; then
  echo 'xe-dpt: requires CONFIG_DRM_XE=m; set calamares.xeDpt.enable = false for this kernel.' >&2
  exit 1
fi
if ! grep -qx 'CONFIG_RANDSTRUCT_NONE=y' "$config"; then
  echo 'xe-dpt: randomized structure layouts are unsupported; set calamares.xeDpt.enable = false.' >&2
  exit 1
fi
if grep -qx 'CONFIG_MODULE_SIG_FORCE=y' "$config"; then
  echo 'xe-dpt: the replacement module is unsigned; set calamares.xeDpt.enable = false when module signatures are required.' >&2
  exit 1
fi
