# SPDX-License-Identifier: GPL-3.0-or-later
# Omarchy's fish configuration, built as omarchy-pkgs' omarchy-fish PKGBUILD
# builds it (https://github.com/omacom/omarchy-pkgs, pkgbuilds/omarchy-fish):
# the same omarchy-fish and fzf.fish releases, whose tarballs match its
# SHA-256 sums, in the vendor directories fish finds through XDG_DATA_DIRS.
# Its bashrc template and omarchy-setup-fish are left out: the catalog module
# hands interactive bash to fish itself.
{
  lib,
  stdenvNoCC,
  fetchFromGitHub,
}:
let
  fzfFish = fetchFromGitHub {
    owner = "PatrickF1";
    repo = "fzf.fish";
    tag = "v10.3";
    hash = "sha256-T8KYLA/r/gOKvAivKRoeqIwE2pINlxFQtZJHpOy9GMM=";
  };
in
stdenvNoCC.mkDerivation (finalAttrs: {
  pname = "omarchy-fish";
  version = "1.5.0";
  src = fetchFromGitHub {
    owner = "omacom-io";
    repo = "omarchy-fish";
    tag = "v${finalAttrs.version}";
    hash = "sha256-WOlyf3BGQxD4Khl+jvbWpajgT7Zyh4+A3PLHJ1jUvzc=";
  };
  dontConfigure = true;
  dontBuild = true;
  installPhase = ''
    runHook preInstall
    fish=$out/share/fish
    # fzf.fish first: omarchy-fish's _fzf_search_history replaces its own.
    for dir in conf.d functions completions; do
      install -Dm644 -t $fish/vendor_''${dir%.d}.d ${fzfFish}/$dir/*.fish
    done
    # dotglob for the functions named ... and ....
    shopt -s dotglob
    for dir in conf.d functions completions; do
      install -Dm644 -t $fish/vendor_''${dir%.d}.d $dir/*.fish
    done
    shopt -u dotglob
    doc=$out/share/omarchy-fish
    install -Dm644 -t $doc LICENSE README.md
    install -Dm644 ${fzfFish}/LICENSE.md $doc/LICENSE.fzf.fish
    install -Dm644 ${fzfFish}/README.md $doc/README.fzf.fish.md
    runHook postInstall
  '';
  meta = {
    description = "Omarchy shell configuration for Fish, with fzf.fish";
    homepage = "https://github.com/omacom-io/omarchy-fish";
    license = lib.licenses.mit;
    platforms = lib.platforms.all;
  };
})
