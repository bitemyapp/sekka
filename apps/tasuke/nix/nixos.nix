# SPDX-License-Identifier: MIT OR Apache-2.0
# Tasuke's NixOS module for systems without a flake:
#
#   imports = [
#     "${builtins.fetchTarball "https://github.com/bitemyapp/sekka/archive/main.tar.gz"}/apps/tasuke/nix/nixos.nix"
#   ];
#   programs.tasuke.enable = true;
import ./module.nix null
