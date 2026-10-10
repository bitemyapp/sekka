# SPDX-License-Identifier: MIT OR Apache-2.0
# Yukimi's NixOS module for systems without a flake:
#
#   imports = [
#     "${builtins.fetchTarball "https://github.com/bitemyapp/sekka/archive/main.tar.gz"}/apps/yukimi/nix/nixos.nix"
#   ];
#   programs.yukimi.enable = true;
import ./module.nix null
