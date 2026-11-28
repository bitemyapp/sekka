# Inspect the actual ACP files shipped to both standalone desktops.
{ pkgs, inputs }:
let
  maxwell = pkgs.callPackage ./audeze-maxwell.nix { };
  standaloneHasProfile =
    desktop:
    let
      system = inputs.nixpkgs.lib.nixosSystem {
        system = pkgs.stdenv.hostPlatform.system;
        modules = [
          inputs.${desktop}.nixosModules.default
          {
            programs.${desktop}.enable = true;
            services.pipewire.enable = true;
          }
        ];
      };
    in
    builtins.elem maxwell.outPath (
      map (package: package.outPath) system.config.services.pipewire.wireplumber.configPackages
    );
in
assert standaloneHasProfile "aizome";
assert standaloneHasProfile "tatami";
pkgs.runCommand "audeze-maxwell-mixer-check" { nativeBuildInputs = [ pkgs.python3 ]; } ''
  python ${./audeze-maxwell-test.py} \
    ${maxwell}/share/wireplumber/wireplumber.conf.d/50-sekka-audeze-maxwell.conf \
    ${pkgs.pipewire}/share/alsa-card-profile/mixer/paths
  touch "$out"
''
