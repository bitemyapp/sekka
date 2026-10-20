# SPDX-License-Identifier: GPL-3.0-or-later
# wpa_supplicant can fail to join a network NetworkManager switches to. A
# scan NetworkManager has just asked for is dropped when it takes the
# interface down to change its scanning address, and wpa_supplicant then
# treats its own scan for the new network as scan-only and joins nothing;
# NetworkManager gives up after 25 s. The patch clears that state when the
# interface goes down. Installed systems and the installation media both
# import this module (DEVELOPERS.md, "Joining Wi-Fi networks").
{ ... }:
{
  nixpkgs.overlays = [
    (final: prev: {
      wpa_supplicant = prev.wpa_supplicant.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [ ./clear-scan-res-handler.patch ];
      });
    })
  ];
}
