# SPDX-License-Identifier: GPL-3.0-or-later
# Omarchy's Starship prompt, with its license, which every shell but bash
# shows unless you have your own ~/.config/starship.toml.
{ lib }:
lib.fileset.toSource {
  root = ./.;
  fileset = lib.fileset.unions [
    ./starship.toml
    ./LICENSE
  ];
}
