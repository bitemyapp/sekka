# SPDX-License-Identifier: GPL-3.0-or-later
# sekka-vpn-guard, with the two programs it reads and changes routes through
# fixed at build time: it runs as root from NetworkManager's dispatcher, and
# takes nothing from PATH.
{
  lib,
  rustPlatform,
  iproute2,
  wireguard-tools,
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
    SEKKA_VPN_GUARD_IP = "${iproute2}/bin/ip";
    SEKKA_VPN_GUARD_WG = "${wireguard-tools}/bin/wg";
  };
  # Unit tests only: the decisions, the order of changes, and parsing of what
  # iproute2 and wg print. Real routes are covered by checks.vpn-guard-vm.
  doCheck = true;
  meta = {
    description = "Blocks the address family a full-tunnel WireGuard connection does not carry";
    license = lib.licenses.gpl3Plus;
    platforms = lib.platforms.linux;
    mainProgram = "sekka-vpn-guard";
  };
}
