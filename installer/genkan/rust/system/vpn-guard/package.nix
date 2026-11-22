# SPDX-License-Identifier: GPL-3.0-or-later
{
  lib,
  rustPlatform,
  nftables,
  systemd,
}:
rustPlatform.buildRustPackage {
  pname = "sekka-vpn-guard";
  version = "0.1.0";
  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
    ];
  };
  cargoLock.lockFile = ./Cargo.lock;
  env = {
    SEKKA_VPN_GUARD_NFT = "${nftables}/bin/nft";
    SEKKA_VPN_GUARD_BUSCTL = "${systemd}/bin/busctl";
  };
  doCheck = true;
  meta = {
    description = "Protects host traffic outside a single-family full-tunnel WireGuard connection";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "sekka-vpn-guard";
  };
}
