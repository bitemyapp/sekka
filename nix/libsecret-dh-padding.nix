# libsecret before 0.21.8 drops leading zero bytes from the Diffie-Hellman
# secret of its GnuTLS backend, so about one Secret Service session in 256
# derives a different key from the keyring's and its secrets fail to store or
# read. GNOME fixed it in 0.21.8 (commit 96a48581), which Nixpkgs has in
# staging. Only the clients Sekka builds itself take the patch: Aizome, whose
# network agent keeps Wi-Fi secrets, and the login test's secret-tool.
# Overriding Nixpkgs' libsecret would rebuild the applications that link it
# (source-built Electron, WebKitGTK, LibreOffice) instead of substituting them,
# as the PipeWire and Avahi fixes avoid too. From 0.21.8 on, this is
# Nixpkgs' own package.
{ lib, libsecret }:
if lib.versionOlder libsecret.version "0.21.8" then
  libsecret.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ./patches/libsecret-gnutls-dh-padding.patch ];
  })
else
  libsecret
