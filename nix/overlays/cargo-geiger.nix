# cargo-geiger 0.13 locks libgit2-sys to libgit2 1.8, while Nixpkgs ships
# libgit2 1.9. Its new Cargo setup hook forbids the bundled compatible copy,
# making a user-installed cargo-geiger block the entire system update.
final: prev: {
  cargo-geiger = prev.cargo-geiger.overrideAttrs (old: {
    buildInputs = (old.buildInputs or [ ]) ++ [
      final.zlib
      final.libssh2
      final.sqlite
    ];
    # Run after cargoUseSystemLibs; env.LIBGIT2_NO_VENDOR is overwritten by
    # that preConfigure hook. Keep system OpenSSL, zlib, libssh2 and SQLite.
    postConfigure = (old.postConfigure or "") + ''
      export LIBGIT2_NO_VENDOR=0
    '';
  });
}
