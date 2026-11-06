# The VM test's controlled update (guest-checks.sh, phase `second`): copied
# to /etc/nixos/setup.nix, the file first-boot setup writes, it adds one
# marker file, so the next generation differs from the image by it and by
# identity.nix. Test only; never part of an image.
{
  environment.etc."sekka-vm-update".text = "generation 2, built offline in the VM test\n";
}
