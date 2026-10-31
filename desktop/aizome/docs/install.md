# Installing Aizome

Aizome is Sekka's desktop, on Sekka's `main` since 20 October 2026 (Sekka
PR #32). Until Sekka's nightly carries it (its 4 AM build moves `nightly`
once its checks pass), a Sekka system installs it from `main`.

## On a Sekka system

1. Add Aizome to the desktops this system offers, and make it the default.
   Check what's there first:

   ```bash
   grep -n 'calamares.desktops\|calamares.defaultDesktop' /etc/nixos/configuration.nix
   ```

   Then put `"aizome"` first in `calamares.desktops`, and set
   `calamares.defaultDesktop` to it:

   ```bash
   sudo sed -i 's/calamares.desktops = \[/calamares.desktops = [ "aizome"/; s/calamares.defaultDesktop = "[^"]*"/calamares.defaultDesktop = "aizome"/' /etc/nixos/configuration.nix
   ```

   If the file has no `calamares.desktops` line, add one listing the desktops
   you have now plus `"aizome"`, for example
   `calamares.desktops = [ "aizome" "plasma" ];`.

2. Build it into the next boot. Sekka refuses to swap the kernel, the login
   screen or D-Bus under a running session, and this update usually changes
   them, so it goes into the next boot instead of switching live:

   ```bash
   sudo nixos-rebuild boot --flake /etc/nixos#$(hostname) --override-input sekka github:bitemyapp/sekka/main
   ```

   Aizome itself isn't in a binary cache yet, so this compiles it, which takes
   a while the first time.

3. Reboot. On the login screen, choose **Aizome**: in GDM, click your name and
   use the gear at the bottom right; in Plasma Login, use the session menu.
   The login screen remembers it from then on.

To update later, run step 2 again: `nixos-rebuild switch` when nothing
session-critical changed, or `boot` and a restart when Sekka says so. Keep
`--override-input sekka github:bitemyapp/sekka/main` until Sekka's nightly
has Aizome (`nix flake metadata github:bitemyapp/sekka/nightly` shows when
it moved); without it, the rebuild stops on the unknown desktop `"aizome"`.

To go back, pick another desktop on the login screen, or boot the previous
generation from the boot menu.

## On another NixOS system with flakes

Add Sekka's flake and its Aizome module to the system's flake:

```nix
{
  inputs.sekka.url = "github:bitemyapp/sekka";
  inputs.sekka.inputs.nixpkgs.follows = "nixpkgs";
  # …
  outputs = { nixpkgs, sekka, ... }: {
    nixosConfigurations.<host> = nixpkgs.lib.nixosSystem {
      modules = [
        sekka.nixosModules.aizome
        { programs.aizome.enable = true; }
        # …
      ];
    };
  };
}
```

`programs.aizome.enable` brings everything Aizome stands on: the session
entry, portals, NetworkManager, BlueZ, PipeWire, UPower, power profiles,
UDisks2, AccountsService, CUPS, GeoClue, GNOME Keyring, polkit, fonts, foot,
and the system's keyboard (`services.xserver.xkb`) as the session's, until
Settings changes it. Hotaru opens fish where it's installed; on a Sekka
system with Aizome, Sekka's modules install it, set up as Omarchy sets it up
(`calamares.fish`). Keep (or enable) a display manager such as
GDM: Aizome adds itself to its session list. Then
`sudo nixos-rebuild boot --flake .#<host>`, reboot and choose Aizome.

## Anywhere else

Install Sekka from its installer, then follow "On a Sekka system".
