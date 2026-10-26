# SPDX-License-Identifier: GPL-3.0-or-later
# Programs built for other Linux distributions run as they would there.
# Editors and version managers fetch their own: Zed its language servers
# (rust-analyzer from GitHub), debugger and agents, VS Code's extensions
# theirs, mise and rustup-style managers whole runtimes. Those binaries ask
# for the usual ELF loader, /lib64/ld-linux-x86-64.so.2, where NixOS has
# only a stub that prints an error. nix-ld puts a loader there that starts
# them with NixOS's glibc and Nixpkgs' set of common libraries, linked in
# /run/current-system/sw/share/nix-ld/lib. Its built-in defaults find both
# without the NIX_LD variables (Zed starts the binaries it downloads with
# its own environment, and a program started from a launcher has no login
# shell's), and before the program starts it puts back the environment it
# was given, so the Nix-built programs it runs, such as cargo, never see
# those libraries. `programs.nix-ld.libraries` adds more libraries;
# `programs.nix-ld.enable = false` brings back the stub.
{ lib, ... }:
{
  programs.nix-ld.enable = lib.mkDefault true;
}
