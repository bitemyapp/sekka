{
  pkgs,
  iso,
  kernel,
}:
let
  verified = import ./verified-iso.nix { inherit pkgs iso; };
  loader = import ./googlebook-vboot-loader.nix { inherit pkgs; };
  command = "$xorriso -output $out/iso/$isoName";
  upstream = builtins.readFile verified.buildCommandPath;
in
assert !iso.bootable && !iso.efiBootable && !iso.usbBootable && !iso.compressImage;
assert pkgs.lib.hasInfix command upstream;
verified.overrideAttrs (old: {
  nativeBuildInputs = old.nativeBuildInputs ++ [
    pkgs.gptfdisk
    pkgs.vboot-utils
    loader
  ];
  # Keep the upstream filesystem builder and mandatory squashfs verification.
  # Only its final xorriso invocation changes; fail evaluation if it moves.
  buildCommandPath = pkgs.writeText "make-googlebook-iso.sh" (
    builtins.replaceStrings
      [ command ]
      [
        ''
          $xorriso -partition_offset 16 \
            -append_partition 2 FE3A2A5D-4F32-41A7-B725-ACCC3285A309 ${kernel}/kernel.vboot \
            -appended_part_as_gpt -output $out/iso/$isoName

          # xorriso emits 248 GPT entries; vboot requires exactly 128. Resizing
          # preserves the ISO and signed payload and repairs both GPT CRCs.
          sgdisk --resize-table=128 "$out/iso/$isoName"
          # A live USB should remain bootable after any number of attempts.
          cgpt add -i 2 -P 15 -T 0 -S 1 -l SEKKA-KERN "$out/iso/$isoName"
          mkdir -p "$out/nix-support"
          load_kernel_test -b 1 "$out/iso/$isoName" \
            ${pkgs.vboot-utils}/share/vboot/devkeys/recovery_key.vbpubk \
            > "$out/nix-support/googlebook-vboot.log" 2>&1 || {
              cat "$out/nix-support/googlebook-vboot.log" >&2
              exit 1
            }
        ''
      ]
      upstream
  );
})
