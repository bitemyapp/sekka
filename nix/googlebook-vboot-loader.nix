# Exercise Google's actual GPT selection and kernel signature verifier on files.
# This is a host test utility, not a bootloader shipped in the live system.
{ pkgs }:
(pkgs.vboot-utils.override {
  withFlashrom = false;
  withNss = false;
}).overrideAttrs
  (old: {
    pname = "sekka-googlebook-vboot-loader";
    postPatch = old.postPatch + ''
      # Match modern x86 Depthcharge's CONFIG_KERNEL_SIZE (512 MiB). Upstream's
      # legacy host test buffer is only 10 MiB, smaller than a modern initramfs.
      substituteInPlace utility/load_kernel_test.c \
        --replace-fail '#define KERNEL_BUFFER_SIZE 0xA00000' '#define KERNEL_BUFFER_SIZE 0x20000000' \
        --replace-fail 'rewind(image_file);' 'rewind(image_file); disk_info.handle = image_file;' \
        --replace-fail '/* Call vb2api_load_kernel() */' \
          '*(enum vb2_boot_mode *)&ctx->boot_mode = (boot_flags & BOOT_FLAG_RECOVERY) ? VB2_BOOT_MODE_MANUAL_RECOVERY : (boot_flags & BOOT_FLAG_DEVELOPER) ? VB2_BOOT_MODE_DEVELOPER : VB2_BOOT_MODE_NORMAL;'
      # The old utility predates the non-null stream handle requirement and
      # ctx->boot_mode. Adapt its harness, leaving firmware verification intact.
    '';
    buildPhase = ''
      runHook preBuild
      make -j"$NIX_BUILD_CORES" USE_FLASHROM=0 DEBUG=1 \
        HOST_ARCH=${pkgs.stdenv.hostPlatform.parsed.cpu.name} "$PWD/build/utility/load_kernel_test"
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      install -Dm755 build/utility/load_kernel_test $out/bin/load_kernel_test
      runHook postInstall
    '';
    postInstall = "";
  })
