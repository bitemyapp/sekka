# Disposable physical inventory smoke, 2026-11-06

The attached [active.out.gz](active.out.gz) contains the unedited stdout of
`build-service physical-inventory --scope active` on `wx-workstation` after
provisioning a fresh, isolated `trusted` service with the imported NativeLink
image. The private manifest and its credentials are not included. A token scan
against the available service manifests found no service credentials in the
receipt. The deployment, its four containers, frontend network, image tag and
private manifest were removed after the observation; no observer containers
remain.

The `sekka.physical-inventory.v1` receipt lists 285 measured roots, four owned
running containers, two referenced image IDs, 47 shared control files and 14
explicitly excluded foreign-platform OCI descriptors. It reports no unresolved
roots. `compsize` returned `TOTAL` physical disk usage of 1,035,043,103 bytes
in one Btrfs filesystem UUID. Its first line is the JSON receipt and the
following lines are the original `compsize` output. SHA-256 of the decompressed stdout:
`302c59355d3927cb49ad65cfadc539b2d4ff2e29d1ec446afb7e6dee900d0683`.

An immutable-scope probe on an existing `owner.json` completed, and an
out-of-allowlist `/tmp` probe failed before creating an observer. This smoke
checks the inventory path and its basic refusal behavior. The measured payload
is a small disposable service, not the complete Buck2/Bazel workload matrix or
an automatic storage limit. The receipt excludes filesystem allocator metadata
and documents the live interval and other scope limits.
