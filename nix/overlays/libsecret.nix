# libsecret 0.21.7's GnuTLS backend omits leading zero bytes in DH secrets.
# This breaks encrypted Secret Service sessions intermittently. Backport the
# upstream fix (included in 0.21.8), for applications as well as the CI probe.
final: prev: {
  libsecret =
    if
      prev.lib.versionOlder prev.libsecret.version "0.21.8"
      && !(prev.libsecret.sekkaDhPaddingFixed or false)
    then
      prev.libsecret.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [ ../patches/libsecret-gnutls-dh-padding.patch ];
        passthru = (old.passthru or { }) // {
          sekkaDhPaddingFixed = true;
        };
      })
    else
      prev.libsecret;
}
