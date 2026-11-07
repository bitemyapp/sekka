# Runtimes for open models on a local NVIDIA GPU, for development work such
# as the agent evaluations (tests/agent-evals). Not shipped in the ISO or
# installed systems, and not among `checks`: CUDA is unfree and these are
# built only for the GPU at hand.
#
#     nix build .#local-models.llama-cpp -o .work/llama-cpp
#     nix run .#local-models.serve -- --help
#
# docs/local-models.md compares the runtimes measured on an RTX 5090.
{
  nixpkgs,
  system,
  # CUDA compute capabilities to build kernels for: 12.0 is the RTX 50
  # series (SM120). Building one architecture keeps the build short.
  cudaCapabilities ? [ "12.0" ],
}:
let
  cudaPkgs = import nixpkgs {
    inherit system;
    config = {
      allowUnfree = true;
      inherit cudaCapabilities;
      cudaForwardCompat = false;
    };
  };
  pkgs = nixpkgs.legacyPackages.${system};
  llama-cpp = cudaPkgs.llama-cpp.override { cudaSupport = true; };
in
{
  inherit llama-cpp;
  # One command for every runtime and model the evaluations use
  # (tests/agent-evals/models.toml): llama.cpp from this flake, vLLM and
  # SGLang from their pinned upstream containers.
  serve = pkgs.writeShellApplication {
    name = "sekka-model-server";
    runtimeInputs = [
      pkgs.python3
      pkgs.curl
      pkgs.coreutils
    ];
    text = ''
      export SEKKA_LLAMA_SERVER=${llama-cpp}/bin/llama-server
      export SEKKA_LLAMA_BENCH=${llama-cpp}/bin/llama-bench
      export SEKKA_MODELS_TOML=''${SEKKA_MODELS_TOML:-${../tests/agent-evals/models.toml}}
      exec python3 -I ${../tests/agent-evals/serve.py} "$@"
    '';
  };
}
