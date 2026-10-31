# Component test: real Linux/vboot packaging and firmware selection, no claim
# that a host test has exercised the ASUS firmware or booted the live desktop.
{
  pkgs,
  googlebook,
  installer,
}:
let
  loader = import ./googlebook-vboot-loader.nix { inherit pkgs; };
  initrd = pkgs.runCommand "googlebook-test-initrd" { nativeBuildInputs = [ pkgs.cpio ]; } ''
    mkdir root
    echo 'Sekka initramfs fixture' > root/marker
    (cd root; printf 'marker\n' | cpio -o -H newc) > $out
  '';
  kernel = import ./googlebook-kernel.nix {
    inherit pkgs initrd;
    kernel = "${pkgs.linux_latest}/bzImage";
    cmdline = "init=/fixture/init console=tty0";
  };
  payload = pkgs.writeText "googlebook-test-payload" "Sekka live store fixture\n";
  raw = pkgs.callPackage "${pkgs.path}/nixos/lib/make-iso9660-image.nix" {
    contents = [
      {
        source = payload;
        target = "/marker";
      }
    ];
    squashfsContents = [ payload ];
    squashfsCompression = "zstd";
    volumeID = "SEKKA_GOOGLEBOOK";
  };
  iso = import ./googlebook-iso.nix {
    inherit pkgs kernel;
    iso = raw;
  };
  cfg = googlebook.config;
in
assert !cfg.respin.enableInstaller;
assert installer.config.respin.enableInstaller;
assert cfg.respin.referenceDesktops == [ ] && !cfg.respin.cacheApplications;
assert cfg.specialisation == { };
assert pkgs.lib.any (p: pkgs.lib.getName p == "genkan") installer.config.environment.systemPackages;
assert pkgs.lib.all (p: pkgs.lib.getName p != "genkan") cfg.environment.systemPackages;
assert pkgs.lib.all (p: !(pkgs.lib.hasInfix "calamares" p.name)) cfg.environment.systemPackages;
assert pkgs.lib.all (m: builtins.elem m cfg.boot.initrd.kernelModules) [
  "coreboot_table"
  "framebuffer_coreboot"
];
pkgs.runCommand "googlebook-iso-test"
  {
    nativeBuildInputs = [
      loader
      pkgs.vboot-utils
      pkgs.xorriso
      pkgs.python3
      pkgs.util-linux
    ];
  }
  ''
    mkdir -p $out
    # Deliberately use a different firmware key: unlocked developer mode must
    # permit the self-signed keyblock while still verifying the signed payload.
    firmware_key=${pkgs.vboot-utils}/share/vboot/devkeys/recovery_key.vbpubk
    cp ${iso}/nix-support/googlebook-vboot.log $out/accepted.log
    xorriso -osirrox on -indev ${iso}/iso/cd.iso -extract /marker extracted
    cmp ${payload} extracted
    dump_kernel_config ${kernel}/kernel.vboot | grep -F 'init=/fixture/init console=tty0'
    python ${../tests/check_googlebook_image.py} ${iso}/iso/cd.iso ${initrd}

    # USB drives are larger than the flashed image: vboot must still accept the
    # primary GPT even when the backup GPT isn't at the physical end of the drive.
    cp ${iso}/iso/cd.iso larger-usb.img
    chmod u+w larger-usb.img
    truncate -s +64M larger-usb.img
    load_kernel_test -b 1 larger-usb.img "$firmware_key" > $out/larger-usb.log 2>&1

    reject() {
      if load_kernel_test -b 1 "$1" "$firmware_key" > "$out/$2.log" 2>&1; then
        echo "Firmware accepted invalid $2 image" >&2
        exit 1
      fi
      grep -F 'vb2api_load_kernel() returned' "$out/$2.log"
    }
    reject ${raw}/iso/cd.iso ordinary-iso
    cp ${iso}/iso/cd.iso disabled.img
    chmod u+w disabled.img
    cgpt add -i 2 -P 0 disabled.img
    reject disabled.img disabled-kernel
    # Flip a signed body byte, leaving valid GPT and keyblock headers intact.
    cp ${iso}/iso/cd.iso corrupt.img
    chmod u+w corrupt.img
    python - <<'PY'
    import struct
    with open('corrupt.img', 'r+b') as f:
        f.seek(2 * 512 + 128 + 32)
        start, = struct.unpack('<Q', f.read(8))
        offset = start * 512 + 0x10000
        f.seek(offset)
        value = f.read(1)[0]
        f.seek(offset)
        f.write(bytes([value ^ 1]))
    PY
    reject corrupt.img corrupt-payload
    grep -F 'Kernel data verification failed' $out/corrupt-payload.log
    if load_kernel_test -b 0 ${iso}/iso/cd.iso "$firmware_key" > $out/locked.log 2>&1; then
      echo 'Accepted developer-signed media in locked mode' >&2
      exit 1
    fi
  ''
