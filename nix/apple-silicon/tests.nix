# Checks of the Mac image's own pieces that need no image build:
# - the installed flake and its generated lock, through Genkan's own
#   Template::parse (the VM fixture's validate-template, as template-lock runs
#   it for PCs), with a lock that adds an input at the root as its failing
#   control, and installed-flake.nix's rendering as exactly the parser's
#   output;
# - boot.bin's verified replacement (boot-bin/test_boot_bin.py);
# - the identity record and boot-entry filling (identity/test_install_identity.py);
# - the initrd's identity check and udev helper (identity/test-check.sh);
# - that the product image shares no name with the developer image;
# - the release manifest against the engine's own reader
#   (release/test_manifest.py).
{
  pkgs,
  lib,
  self,
}:
let
  installed = import ./installed-flake.nix { inherit lib self; };
  # Genkan's template check, for any system's fixture: flake.nix for x86_64
  # CI uses it with the PC fixture (checks.x86_64-linux.apple-installed-template).
  templateCheck =
    { pkgs, fixture }:
    pkgs.runCommand "apple-installed-template"
      {
        nativeBuildInputs = [ pkgs.python3 ];
        template = installed.template;
        lock = builtins.toJSON installed.lock;
        named = installed.named "sekka-test";
        passAsFile = [
          "template"
          "lock"
          "named"
        ];
      }
      ''
        cp $templatePath template.nix.in
        cp $lockPath flake.lock
        # The real backend parser accepts the rendered template and lock...
        ${fixture}/bin/calamares-vm-fixture validate-template template.nix.in flake.lock > parsed.nix
        # ...and writes, for a host name, exactly what installed-flake.nix
        # renders, from which the shipped flake (named by the configuration's
        # host name; checked at evaluation) differs only in that name.
        cmp parsed.nix $namedPath
        python3 - <<'EOF'
        import json
        lock = json.load(open("flake.lock"))
        root = lock["nodes"][lock["root"]]["inputs"]
        followed = {"nixpkgs", "determinate", "fh", "applications", "ai-apps", "omp"}
        assert set(root) == followed | {"sekka"}, root
        sekka = lock["nodes"]["sekka"]
        assert sekka["original"] == {"owner": "bitemyapp", "repo": "sekka", "type": "github",
                                     "ref": "${installed.ref}"}, sekka["original"]
        # Implementation-only inputs stay beneath Sekka.
        assert {"superluminal", "tracy", "uta"} <= set(sekka["inputs"]), sekka["inputs"]
        # The failing control: an implementation input at the root.
        lock["nodes"][lock["root"]]["inputs"]["superluminal"] = ["sekka", "superluminal"]
        json.dump(lock, open("extra.lock", "w"))
        EOF
        if ${fixture}/bin/calamares-vm-fixture validate-template template.nix.in extra.lock >/dev/null 2>err; then
          echo "Template::parse accepted a lock with an extra root input" >&2
          exit 1
        fi
        grep -q "Unexpected target flake inputs" err
        mkdir $out
        cp parsed.nix flake.lock $out/
      '';
  template = templateCheck {
    inherit pkgs;
    fixture = import ../fixture.nix { inherit pkgs; };
  };
  scripts =
    pkgs.runCommand "apple-image-scripts"
      {
        nativeBuildInputs = [
          pkgs.python3
          pkgs.bash
          pkgs.coreutils
          pkgs.gnugrep
          pkgs.gnused
        ];
      }
      ''
        cp ${./boot-bin}/*.py ${./identity}/*.py .
        python3 -m unittest -v test_boot_bin test_install_identity 2>&1 | tee unittest.log
        bash ${./identity/test-check.sh} ${./identity} | tee check.log
        # A developer install mounts by the developer image's names
        # (developer.nix, as the original J514S installation's configuration
        # does); the product image has other ones and mounts by none.
        python3 - ${./identities.json} ${./developer/developer.nix} ${./product/configuration.nix} <<'EOF'
        import json, sys
        ids = json.load(open(sys.argv[1]))
        developer, product = open(sys.argv[2]).read(), open(sys.argv[3]).read()
        d, p = ids["developer"], ids["product"]
        serial = "%04X-%04X" % divmod(int(d["esp_volume_id"], 16), 0x10000)
        assert f'"/dev/disk/by-label/{d["root_label"]}"' in developer, "developer.nix's root"
        assert f'"/dev/disk/by-uuid/{serial}"' in developer, "developer.nix's ESP"
        assert p["root_label"] != d["root_label"] and p["root_uuid"] != d["root_uuid"], p
        assert p["esp_volume_id"] is None, p
        for name in (d["root_label"], d["root_uuid"], serial, p["root_label"], p["root_uuid"]):
            assert name not in product, f"configuration.nix names {name}"
        print("identities: the product image shares no name with the developer image")
        EOF
        mkdir $out
        cp unittest.log check.log $out/
      '';
  # release/manifest.py's output for a small release, read by the engine's
  # own sekka_engine.assets (release/test_manifest.py): the manifest is
  # the engine's contract, and only the engine's sources are copied in.
  manifest =
    pkgs.runCommand "apple-image-manifest"
      {
        nativeBuildInputs = [ pkgs.python3 ];
      }
      ''
        mkdir engine work
        cp -r ${../../installer/genkan/macos/engine/sekka_engine} engine/sekka_engine
        python3 ${./release/test_manifest.py} ${./release/manifest.py} engine ${./identities.json} work \
          | tee manifest.log
        mkdir $out
        cp manifest.log work/manifest.json $out/
      '';
in
{
  inherit
    templateCheck
    template
    scripts
    manifest
    ;
  all = pkgs.linkFarm "mac-image-tests" [
    {
      name = "template";
      path = template;
    }
    {
      name = "scripts";
      path = scripts;
    }
    {
      name = "manifest";
      path = manifest;
    }
  ];
}
