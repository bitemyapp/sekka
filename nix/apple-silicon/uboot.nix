# U-Boot for Apple Silicon (AsahiLinux/u-boot asahi-v2026.07-2), configured as
# nix-community/nixos-apple-silicon configures it (MIT). m1n1 starts it from
# boot.bin, and it starts systemd-boot from the EFI system partition.
{ buildUBoot, fetchFromGitHub }:
(buildUBoot rec {
  version = "2026.07-2-asahi";
  src = fetchFromGitHub {
    owner = "AsahiLinux";
    repo = "u-boot";
    rev = "asahi-v2026.07-2";
    hash = "sha256-9E79VceZcPw55zj6BBwgIBP/2OrHRS/w3ypGyOWfE6U=";
  };
  defconfig = "apple_m1_defconfig";
  extraMeta.platforms = [ "aarch64-linux" ];
  filesToInstall = [ "u-boot-nodtb.bin.gz" ];
  extraConfig = ''
    CONFIG_IDENT_STRING=" ${version}"
    CONFIG_VIDEO_FONT_4X6=n
    CONFIG_VIDEO_FONT_8X16=n
    CONFIG_VIDEO_FONT_SUN12X22=n
    CONFIG_VIDEO_FONT_16X32=y
    CONFIG_CMD_BOOTMENU=y
  '';
}).overrideAttrs
  (_: {
    # Nixpkgs' U-Boot patches are for upstream U-Boot.
    patches = [ ];
    # Compressed, so that m1n1 knows its size and finds what follows it.
    preInstall = "gzip -n u-boot-nodtb.bin";
  })
