# Q8 — workstation publication and R2 pruning

Publication must run on the user's workstation after exact-image testing. CI
may build, test and upload binary-cache paths; it must not upload ISOs or hold
`sekka-downloads` write credentials. Inspect the actual default-branch workflow
and active runs, not just a local patch. Keep `CACHE_*` credentials intact.

## Candidate review

Reconcile every Q0–Q7 ID and dynamically inventoried feature. Confirm all
required checks passed on the same SHA-256 and source. An independent agent
reads the evidence, reproduces representative critical paths and checks missing
coverage. Record concrete limitations; untested install/boot/login behavior is
not a publishable limitation. Preserve a machine-readable report and screenshots
with no private credentials or personal data.

Check that source is clean and corresponds to the built image and dependency
lock. Export the final ledger using the [report v1 contract](report.md), including
its independent audit. The uploader verifies the exact clean Git revision's
evaluated Nix output, candidate hash/size, dependency lock and workstation, and
rehashes immediately before transfer. Never rebuild after testing and assume
the new bytes are equivalent. Structural validation cannot judge what evidence
proves or whether the source feature inventory is complete; review remains
mandatory.

Use local environment/profile credentials supplied for the downloads bucket.
Print key names only when locating them; do not log values or pass them into
VMs, command arguments, tracked files, reports or PRs. Existing session
publication/pruning authorization applies; do not ask again for permission
already granted. The skill alone does not authorize an unrelated publication.

## Removing broken published builds

1. Save `builds.json`, per-build manifests/checksums and a complete paginated
   object listing for `nightly/`, including orphaned ISO objects. Do not touch
   the separate binary-cache bucket.
2. Match each ISO to its hash, source and reproduced failure. Preserve local
   evidence for removed builds. A lack of tests is "unverified", not proof of
   failure; arrange testing or an explicitly authorized quarantine.
3. Compute and inspect the exact new index and deletion list. Remove failed
   entries from the index first, preserving healthy entries and schema. If all
   builds fail, publish `{"builds":[]}`; an honest empty list is preferable to
   linking known-broken downloads, but it does not finish the user's goal.
4. Upload the corrected index with `Cache-Control: no-cache`. Delete only the
   identified failed build's exact objects, including ISO, sums, lock and
   manifest; no unreviewed recursive prefix wipe. Re-list to prove removal.
5. Fetch the public index and download page. The site worker caches the index
   for up to five minutes. Verify the actual rendered links after expiration,
   and verify deleted ISO URLs fail. Do not declare removal from an API's
   success response alone. Retained builds still require qualification.

## Publishing a qualified image

Use the root tool only after review above, from the clean candidate checkout:

```sh
nix run .#publish-nightly -- validate ISO_DIRECTORY QUALIFICATION_DIRECTORY
nix run .#publish-nightly -- publish ISO_DIRECTORY QUALIFICATION_DIRECTORY
```

Validation makes no R2 requests. Publication refuses CI, an incomplete report,
stale audit or candidate, missing/tampered evidence, and any occupied date/revision
prefix. It conditionally reserves `publication.json` so concurrent publishers
cannot claim the same immutable path. An incomplete or uncertain upload remains
unindexed and reserved for inspection; do not delete its claim merely to retry.
The ISO, checksum, exact lock, source manifest and listed qualification evidence
are uploaded and checked before an atomic conditional update of `builds.json`.
An index conflict requires review of the new remote state; it is never blindly
retried. Retention removes only exact object paths named by the dropped builds
in that previously read index; unindexed objects are preserved for investigation.
Do not reuse an immutable path after any object has been publicly served.

Confirm remote object size/checksum metadata, then independently download the
published bytes and check the SHA-256. Fetch the public page and each linked
manifest/checksum/lock/ISO. Verify Kakehashi can read the index. The public
manifest must identify the qualified image and make evidence discoverable.
Record final index, object list, page response, PR/merge revisions and post-upload
verification. Only then mark the public-download objective complete.
