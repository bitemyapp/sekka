# Checks of the Mac image's own pieces that need no image build:
# - the installed flake and its generated lock, through Genkan's own
#   Template::parse (the VM fixture's validate-template, as template-lock runs
#   it for PCs), with a lock that adds an input at the root as its failing
#   control, and installed-flake.nix's rendering as exactly the parser's
#   output;
# - boot.bin's verified replacement (boot-bin/test_boot_bin.py);
# - the identity record and boot-entry filling (identity/test_install_identity.py);
# - the initrd's identity check and udev helper (identity/test-check.sh).
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
        mkdir $out
        cp unittest.log check.log $out/
      '';
in
{
  inherit templateCheck template scripts;
  all = pkgs.linkFarm "mac-image-tests" [
    {
      name = "template";
      path = template;
    }
    {
      name = "scripts";
      path = scripts;
    }
  ];
}
