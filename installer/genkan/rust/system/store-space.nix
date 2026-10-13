# SPDX-License-Identifier: GPL-3.0-or-later
# Nix collects garbage by itself when a build or download leaves less than
# 5 GB free, until 20 GB is: only store paths nothing refers to, never a
# generation in the boot menu or a project's pinned shell. It sits under
# Yukimi's own check, which stops an update below 4 GB, so neither an update
# nor anything else Nix does fills the disk.
{ lib, ... }:
{
  nix.settings = {
    min-free = lib.mkDefault 5000000000;
    max-free = lib.mkDefault 20000000000;
  };
}
