# Backport https://github.com/hyprwm/aquamarine/commit/fc77bd58a2c9ef6d0760a7c9c3b7b6670f956f28
# to Aquamarine before 0.16.0, which includes this teardown crash fix. Sekka's
# own lock has 0.16.0, so this returns Nixpkgs' package unchanged; the backport
# remains for flakes whose Sekka follows an older Nixpkgs of their own. Do not
# silently skip a failed patch when updating inputs. The null-connector check
# must still pass.
final: prev: {
  aquamarine =
    if final.lib.versionOlder prev.aquamarine.version "0.16.0" then
      prev.aquamarine.overrideAttrs (old: {
        patches = (old.patches or [ ]) ++ [ ../patches/aquamarine-null-connector.patch ];
      })
    else
      prev.aquamarine;
}
