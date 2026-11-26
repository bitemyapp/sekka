# BlueZ's virtual controller (btvirt), which nixpkgs' BlueZ doesn't
# install: BlueZ built with its testing tools, keeping only btvirt. For the
# VM checks, over hci_vhci.
{ pkgs }:
let
  bluez = pkgs.bluez.overrideAttrs (old: {
    pname = "bluez-testing";
    configureFlags = old.configureFlags ++ [ "--enable-testing" ];
    doCheck = false;
    postInstall = old.postInstall + ''
      install -Dm755 emulator/btvirt $out/bin/btvirt
    '';
  });
in
pkgs.runCommand "btvirt" { } "install -Dm755 ${bluez}/bin/btvirt $out/bin/btvirt"
