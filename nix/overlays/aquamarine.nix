# Backport https://github.com/hyprwm/aquamarine/commit/fc77bd58a2c9ef6d0760a7c9c3b7b6670f956f28
# Nixpkgs currently ships 0.15.1, which predates this teardown crash fix.
# Remove this backport when Nixpkgs incorporates it; do not silently skip a
# failed patch when updating inputs. The null-connector check must still pass.
final: prev: {
  aquamarine = prev.aquamarine.overrideAttrs (old: {
    patches = (old.patches or [ ]) ++ [ ../patches/aquamarine-null-connector.patch ];
  });
}
