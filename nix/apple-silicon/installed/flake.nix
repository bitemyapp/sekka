# Installed configuration for the original Sekka J514S developer image.
# Follow the Apple Silicon integration branch until its platform is on main.
{
  inputs.sekka.url = "github:bitemyapp/sekka/fix/apple-silicon-aizome";
  inputs.nixpkgs.follows = "sekka/nixpkgs";
  inputs.determinate.follows = "sekka/determinate";
  inputs.fh.follows = "sekka/fh";
  inputs.applications.follows = "sekka/applications";
  inputs.ai-apps.follows = "sekka/ai-apps";
  inputs.omp.follows = "sekka/omp";

  outputs =
    {
      nixpkgs,
      determinate,
      fh,
      applications,
      ai-apps,
      omp,
      sekka,
      ...
    }:
    {
      nixosConfigurations.sekka = nixpkgs.lib.nixosSystem {
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
