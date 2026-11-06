# The VM test's controlled update (guest-checks.sh, phase `second`): copied
# to /etc/nixos and imported from configuration.nix, as an owner's change
# would be, it adds one marker file, so the next generation differs from the
# image by it and by identity.nix. Test only; never part of an image.
{
  environment.etc."sekka-vm-update".text = "generation 2, built offline in the VM test\n";
}
