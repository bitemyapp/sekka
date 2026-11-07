# What first-boot setup changes of the Mac image's configuration.nix
# (nix/apple-silicon/product/), in the same form: the default desktop, a
# plain string in the `calamares` set, which setup and Yukimi change in
# place. configuration.nix is this as the image ships it, and
# configuration.after.nix as setup leaves it for Tatami (a helper test holds
# it to that); the configuration check evaluates both.
{
  calamares = {
    desktops = [
      "aizome"
      "tatami"
    ];
    defaultDesktop = "tatami";
  };
}
