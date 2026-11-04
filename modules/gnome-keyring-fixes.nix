# NixOS takes the keyring daemon, its PAM module and its D-Bus service all
# from pkgs.gnome-keyring, so the fix replaces that package for the system.
{
  nixpkgs.overlays = [ (import ../nix/overlays/gnome-keyring.nix) ];
}
