# SPDX-License-Identifier: MIT OR Apache-2.0
# Tasuke's headless parts: the `tasuke` command and `tasuke-helper` (run as
# root through pkexec), with the polkit action that names the helper and the
# skills. The window (aizome-tasuke) is built in Aizome's workspace.
{
  lib,
  rustPlatform,
  bash,
}:
let
  # Content-addressed source: the same derivation wherever the checkout is.
  source = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../rustfmt.toml
      ../crates
      ../tasuke-cli
      ../tasuke-helper
      # The model catalog is compiled in; the skills are read at run time.
      ../catalog
      ../skills
    ];
  };
in
rustPlatform.buildRustPackage {
  pname = "tasuke";
  version = "0.1.0";
  src = source;
  cargoLock.lockFile = ../Cargo.lock;
  # The agent finds its helper here, the path the polkit action names.
  env.TASUKE_HELPER_PATH = "${placeholder "out"}/libexec/tasuke-helper";
  # Root commands run in the system's shell, from this package.
  env.TASUKE_BASH = "${bash}/bin/bash";
  postInstall = ''
    # Only through pkexec, under its own action, which names it by this path.
    mkdir -p $out/libexec $out/share/polkit-1/actions $out/share/tasuke
    mv $out/bin/tasuke-helper $out/libexec/
    substitute tasuke-helper/data/io.github.bitemyapp.Tasuke.policy.in \
      $out/share/polkit-1/actions/io.github.bitemyapp.Tasuke.policy \
      --subst-var-by helper $out/libexec/tasuke-helper
    cp -r skills $out/share/tasuke/skills
  '';
  meta = {
    description = "Tasuke, Sekka's private local assistant: models, model server, root helper and CLI";
    homepage = "https://github.com/bitemyapp/sekka/tree/main/apps/tasuke";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "tasuke";
    platforms = lib.platforms.linux;
  };
}
