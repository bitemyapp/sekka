{ pkgs }:
let
  verify = import ./verify-squashfs.nix { inherit pkgs; };
  payload = pkgs.runCommand "iso-integrity-payload" { } ''
    mkdir -p $out/empty
    echo original > $out/data
    echo executable > $out/program
    chmod +x $out/program
    ln -s data $out/link
  '';
  closure = pkgs.closureInfo { rootPaths = [ payload ]; };
  # Exercise the same upstream ISO builder and mandatory wrapper as .#iso.
  iso = import ./verified-iso.nix {
    inherit pkgs;
    iso = pkgs.callPackage "${pkgs.path}/nixos/lib/make-iso9660-image.nix" {
      contents = [ ];
      squashfsContents = [ payload ];
      squashfsCompression = "zstd";
      volumeID = "SEKKA_TEST";
    };
  };
in
pkgs.runCommand "iso-store-integrity"
  {
    nativeBuildInputs = [
      verify
      pkgs.squashfsTools
      pkgs.gnugrep
    ];
  }
  ''
    test -f ${iso}/iso/cd.iso
    mkdir fixture
    cp ${closure}/registration fixture/nix-path-registration
    name=$(basename ${payload})
    reset_payload() {
      chmod -R u+w fixture
      rm -rf "fixture/$name"
      cp -a ${payload} "fixture/$name"
      chmod -R u+w "fixture/$name"
    }
    pack() {
      rm -f image.squashfs
      mksquashfs fixture image.squashfs -noappend -no-progress -processors 1
    }
    reject() {
      # The old packaging succeeds even with corrupt content; the gate must fail.
      pack
      if sekka-verify-squashfs image.squashfs ${closure}/registration > "$1.log" 2>&1; then
        echo "Accepted corrupt $1" >&2
        exit 1
      fi
      grep -E 'modified|corrupt|does not exist|No such file|differ|EOF' "$1.log"
    }
    reset_payload
    pack
    sekka-verify-squashfs image.squashfs ${closure}/registration
    echo changed > "fixture/$name/data"
    reject contents
    reset_payload
    chmod -x "fixture/$name/program"
    reject executable
    reset_payload
    ln -sfn other "fixture/$name/link"
    reject symlink
    reset_payload
    rmdir "fixture/$name/empty"
    reject directory
    reset_payload
    rm -rf "fixture/$name"
    reject missing
    reset_payload
    : > fixture/nix-path-registration
    reject registration
    mkdir $out
    cp ./*.log $out/
  ''
