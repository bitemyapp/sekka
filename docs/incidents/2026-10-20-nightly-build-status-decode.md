# Nightly failure hidden behind `Invalid BuildResult status ^P`

The 2026-10-20 nightly. Component: the release runner's Nix client. Fix:
[#83](https://github.com/bitemyapp/sekka/pull/83); the underlying build
failure: [#59](https://github.com/bitemyapp/sekka/pull/59).

## Report

The October 20 nightly failed with `Invalid BuildResult status ^P` instead of
the failing build's own error. The real failure was a template-lock failure.

## Cause

The runner's builder used an upstream Nix client against the host's
Determinate daemon. When one build fails, the daemon cancels the builds
running beside it. Determinate 3.15.0 encodes a cancelled build as
[status 16](https://github.com/DeterminateSystems/nix-src/blob/v3.15.0/src/libstore/include/nix/store/build-result.hh),
while the upstream client's
[decoder accepts only 0–14](https://github.com/NixOS/nix/blob/2.34.8/src/libstore/common-protocol.cc),
so the client failed decoding the cancellation and the original error was
lost. The developer notes named the client as upstream 2.34.8; #83 records
2.34.9 against Determinate 3.15.0 from a later read-only inspection of
production.

## Fix

- The runner's Nix client comes from Sekka's pinned Determinate input,
  matching the host daemon's implementation.
- The builder step prints the client version and only the daemon endpoint,
  version and trust status, not environment variables or configuration.
- The template-lock defect itself was fixed in #59.

## Checks

`checks.x86_64-linux.runner-protocol` runs a real daemon in a VM as an
untrusted user. The upstream client reproduces the decoding failure (the
negative control); the pinned client reports the failed derivation, cancels
the other build and leaves no child build running. The runner images were
built and their PATH resolves to the pinned client. No runner was redeployed
and no workflow was dispatched as part of the fix; the check doesn't certify
an existing runner deployment.
