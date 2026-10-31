{
  pkgs,
  kernel,
  initrd,
  cmdline,
}:
pkgs.runCommand "sekka-googlebook-kernel" { nativeBuildInputs = [ pkgs.depthcharge-tools ]; } ''
  mkdir -p $out
  # Public developer keys: requires an unlocked bootloader with external boot
  # allowed. These are not Google production signatures or Secure Boot keys.
  # mkdepthcharge places the initramfs in the signed payload, fixes its x86 boot
  # parameters and reserves decompression space without disabling KASLR.
  mkdepthcharge --arch x86_64 --format zimage --kernel-start 0x100000 \
    --keydir ${pkgs.vboot-utils}/share/vboot/devkeys \
    --cmdline ${pkgs.lib.escapeShellArg cmdline} \
    --output $out/kernel.vboot -- ${kernel} ${initrd}
  # Modern x86 Depthcharge's kernel buffer. Fail rather than silently generate
  # media the firmware cannot load as the live initramfs grows.
  test "$(stat -c %s $out/kernel.vboot)" -le 536870912
''
