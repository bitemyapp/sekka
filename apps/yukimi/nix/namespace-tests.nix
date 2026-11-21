# SPDX-License-Identifier: MIT OR Apache-2.0
# The helper's namespace tests (yukimi-helper/tests/namespace.rs), which a
# plain `cargo test` leaves ignored: the real worker against stand-ins for
# Nix. From the helper package's own source and vendored crates, as a check
# of its own, so the package builds where user namespaces can't nest.
#
# Each script mounts its private /run, /nix/var/nix, /var/lib/yukimi,
# /etc/yukimi and /etc/nixos over directories that exist. A build sandbox
# has none of them and its root can't take them, so bubblewrap gives the
# tests a root that does: the store read-only, the build directory, and
# those five empty. The scripts then make their own namespace inside it, as
# they do on a developer's machine.
{
  helper,
  bubblewrap,
}:
helper.overrideAttrs (old: {
  pname = "yukimi-helper-namespace-tests";
  nativeCheckInputs = old.nativeCheckInputs ++ [ bubblewrap ];
  # The package's check has run the lints and the other tests.
  preCheck = "";
  dontBuild = true;
  doCheck = true;
  # --include-ignored, so the tests still run here should they stop being
  # ignored.
  checkPhase = ''
    runHook preCheck
    tests=(cargo test --offline --locked --profile release -p yukimi-helper --test namespace)
    "''${tests[@]}" --no-run
    bwrap --unshare-all --die-with-parent \
      --proc /proc --dev /dev --tmpfs /tmp \
      --ro-bind /nix/store /nix/store \
      --bind "$NIX_BUILD_TOP" "$NIX_BUILD_TOP" \
      --dir /run --dir /nix/var/nix --dir /var/lib/yukimi --dir /etc/yukimi --dir /etc/nixos \
      -- "''${tests[@]}" -- --include-ignored
    runHook postCheck
  '';
  installPhase = ''
    touch $out
  '';
  dontFixup = true;
})
