# The files beside configuration.nix that exist only on some Macs, imported
# when they do (configuration.nix says what each is). They are kept out of
# configuration.nix so that its `imports` is a plain list Yukimi can add to.
{
  imports = builtins.filter builtins.pathExists [
    ./identity.nix
    ./setup.nix
    ./developer.nix
  ];
}
