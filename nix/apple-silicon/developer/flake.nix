# Shipped with Sekka's Mac image (nix/apple-silicon). Update Sekka through Yukimi or `sekka-update`.
# This lock records the exact Sekka source and dependencies the image was built from.
# Everything else follows Sekka: the revisions it was built and tested with,
# which updating Sekka moves together.
{
  inputs.sekka.url = "github:bitemyapp/sekka/fix/apple-silicon-aizome";
  inputs.nixpkgs.follows = "sekka/nixpkgs";
  inputs.determinate.follows = "sekka/determinate";
  inputs.fh.follows = "sekka/fh";
  inputs.applications.follows = "sekka/applications";
  inputs.ai-apps.follows = "sekka/ai-apps";
  inputs.omp.follows = "sekka/omp";

  outputs = { nixpkgs, determinate, fh, applications, ai-apps, omp, sekka, ... }: {
    nixosConfigurations."sekka" = nixpkgs.lib.nixosSystem {
      system = "aarch64-linux";
      specialArgs = {
        applicationPkgs = import applications {
          system = "aarch64-linux";
          config.allowUnfree = true;
        };
        aiPackages = ai-apps.packages.aarch64-linux;
        ompPackage = omp.packages.aarch64-linux.omp;
      };
      modules = [
        determinate.nixosModules.default
        sekka.nixosModules.default
        ./configuration.nix
        ({ pkgs, ... }: {
          environment.systemPackages = [ fh.packages.${pkgs.stdenv.hostPlatform.system}.default ];
        })
      ];
    };
  };
}
