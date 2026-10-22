# Zed as zed.dev installs it on Ubuntu: Zed's own release in
# ~/.local/zed.app, which updates itself in place and downloads its own
# language servers, Node.js and extensions' tools. Those run through the
# system's foreign-binaries module (installer/genkan/rust/system).
#
# Nixpkgs' zed-editor is built with updates switched off, and the store it
# lives in is read-only. Here the store holds only the release Zed starts
# from: the first launch copies it into the account, offline, and from then
# on the copy is Zed's to replace. A newer copy is never touched.
{
  lib,
  stdenvNoCC,
  fetchurl,
  writeShellApplication,
  coreutils,
  gnutar,
  gzip,
  rsync,
}:
let
  version = "1.25.2";
  release = fetchurl {
    url = "https://github.com/zed-industries/zed/releases/download/v${version}/zed-linux-x86_64.tar.gz";
    sha256 = "023mc7372r3jaz5hdqxps4nz5q9shnrffca6hn2gz0hch7mfn1dl";
  };
  launcher = writeShellApplication {
    name = "zed";
    text = ''
      app="$HOME/.local/zed.app"
      if [ ! -x "$app/bin/zed" ] || [ ! -x "$app/libexec/zed-editor" ]; then
        ${coreutils}/bin/mkdir -p "$HOME/.local"
        unpacked=$(${coreutils}/bin/mktemp -d "$HOME/.local/.zed.app.XXXXXX")
        PATH=${gzip}/bin ${gnutar}/bin/tar -xzf ${release} -C "$unpacked"
        ${coreutils}/bin/rm -rf "$app"
        ${coreutils}/bin/mv "$unpacked/zed.app" "$app"
        ${coreutils}/bin/rmdir "$unpacked"
      fi
      # Zed's updater unpacks with tar and installs with rsync, as on Ubuntu,
      # where both come with the system. After the account's own programs.
      export PATH="''${PATH:+$PATH:}${
        lib.makeBinPath [
          gnutar
          gzip
          rsync
        ]
      }"
      exec "$app/bin/zed" "$@"
    '';
  };
in
stdenvNoCC.mkDerivation {
  pname = "zed";
  inherit version;
  src = release;
  dontConfigure = true;
  dontBuild = true;
  dontFixup = true;
  installPhase = ''
    runHook preInstall
    mkdir -p $out/bin $out/share/applications
    ln -s ${lib.getExe launcher} $out/bin/zed
    cp -r share/icons $out/share/
    sed -e "s|^TryExec=zed$|TryExec=$out/bin/zed|" \
      -e "s|^Exec=zed |Exec=$out/bin/zed |" \
      share/applications/dev.zed.Zed.desktop > $out/share/applications/dev.zed.Zed.desktop
    runHook postInstall
  '';
  passthru = { inherit release launcher; };
  meta = {
    description = "Zed's own release, installed in the account so that it updates itself";
    homepage = "https://zed.dev";
    license = lib.licenses.gpl3Only;
    mainProgram = "zed";
    platforms = [ "x86_64-linux" ];
  };
}
