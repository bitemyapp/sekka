# sekka.computer: static pages built by the Rust generator in site/ from the
# brand tokens, logos and screenshots in this repository, with the fonts it
# serves taken from this Nixpkgs, and the download worker
# (site/download-worker), which Pages runs for /download/ as _worker.js. The
# output is the directory to deploy, with Wrangler (DEVELOPERS.md).
{
  pkgs,
  revision,
  date,
}:
let
  inherit (pkgs) lib;
  generator = pkgs.rustPlatform.buildRustPackage {
    pname = "sekka-site";
    version = "0.1.0";
    src = lib.fileset.toSource {
      root = ../site;
      fileset = lib.fileset.unions [
        ../site/Cargo.toml
        ../site/Cargo.lock
        ../site/src
        ../site/style.css
        ../site/theme.js
      ];
    };
    cargoLock.lockFile = ../site/Cargo.lock;
    meta.mainProgram = "sekka-site";
  };

  # The download worker, from the site's sources it shares (src/shared.rs).
  workerSource = lib.fileset.toSource {
    root = ../site;
    fileset = lib.fileset.unions [
      ../site/download-worker/Cargo.toml
      ../site/download-worker/Cargo.lock
      ../site/download-worker/src
      ../site/download-worker/pages-shim.js
      ../site/src/shared.rs
    ];
  };
  workerLock = ../site/download-worker/Cargo.lock;

  # Its rendering is plain Rust, tested here on the build machine.
  workerTests = pkgs.rustPlatform.buildRustPackage {
    pname = "sekka-download-worker-tests";
    version = "0.1.0";
    src = workerSource;
    buildAndTestSubdir = "download-worker";
    cargoRoot = "download-worker";
    cargoLock.lockFile = workerLock;
    installPhase = "touch $out";
  };

  # workers-rs' own builder, at the version of the worker crate it pairs
  # with: it compiles the crate for Wasm, runs wasm-bindgen, and bundles the
  # JavaScript that loads it with esbuild. Here it uses these tools rather
  # than downloading its own; wasm-opt is off (Cargo.toml says why).
  workerBuild = pkgs.rustPlatform.buildRustPackage rec {
    pname = "worker-build";
    version = "0.8.5";
    src = pkgs.fetchCrate {
      inherit pname version;
      hash = "sha256-xJjClsC++Kuh6rKGaYRuMdZtHDgJgsyfjtqgH9zDpTk=";
    };
    cargoHash = "sha256-FGZXALvFydgPSxdApiO9WexjlHUSXmc/erH/F/q8/Z0=";
    nativeBuildInputs = [ pkgs.pkg-config ];
    buildInputs = [ pkgs.openssl ];
    doCheck = false;
  };
  worker = pkgs.stdenv.mkDerivation {
    name = "sekka-download-worker";
    src = workerSource;
    cargoRoot = "download-worker";
    cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = workerLock; };
    nativeBuildInputs = [
      pkgs.rustPlatform.cargoSetupHook
      pkgs.cargo
      pkgs.rustc
      pkgs.lld
      workerBuild
    ];
    # Exactly the version the crate's Cargo.lock holds.
    WASM_BINDGEN_BIN = lib.getExe' pkgs.wasm-bindgen-cli_0_2_127 "wasm-bindgen";
    ESBUILD_BIN = lib.getExe pkgs.esbuild;
    # The loader Pages needs (the file says why), relative to the crate.
    CUSTOM_SHIM = "pages-shim.js";
    buildPhase = ''
      runHook preBuild
      export HOME=$TMPDIR
      cd download-worker
      worker-build --release
      runHook postBuild
    '';
    installPhase = ''
      runHook preInstall
      mkdir $out
      cp build/worker/shim.mjs $out/index.js
      cp build/worker/index.wasm $out/index.wasm
      runHook postInstall
    '';
  };

  # Only what the pages show: changing anything else leaves the site alone.
  content = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../docs/brand/tokens.json
      ../docs/brand/logos
      ../docs/brand/family
      ../docs/images/sekka
      ../apps/yukimi/yukimi/data/io.github.bitemyapp.Yukimi.svg
    ];
  };
  mono = pkgs.jetbrains-mono;
  sans = pkgs.atkinson-hyperlegible-next;
in
pkgs.runCommand "sekka-site" { } ''
  ${lib.getExe generator} \
    --repo ${content} \
    --out $out \
    --revision ${lib.escapeShellArg revision} \
    --date ${lib.escapeShellArg date} \
    --mono "${mono}/share/fonts/truetype/JetBrainsMono[wght].ttf" \
    --mono-license ${mono.src}/OFL.txt \
    --sans "${sans}/share/fonts/truetype/AtkinsonHyperlegibleNext[wght].ttf" \
    --sans-italic "${sans}/share/fonts/truetype/AtkinsonHyperlegibleNext-Italic[wght].ttf" \
    --sans-license ${sans.src}/OFL.txt \
    --cjk ${pkgs.noto-fonts-cjk-serif}/share/fonts/opentype/noto-cjk/NotoSerifCJK-VF.otf.ttc
  # Pages runs _worker.js/index.js, which loads the module beside it.
  mkdir $out/_worker.js
  cp ${worker}/index.js ${worker}/index.wasm $out/_worker.js/
  : ${workerTests}
''
