# Native automatic URL and path detection candidate

Frozen candidate based on shared8f05f4a6. No shared product edits were made by
this prototype. All compilation, tests, actual reference execution and Wayland
runs occurred on wx-workstation under the shared workstation lock. The final
package has not yet been built or qualified by the parent integration.

`candidate.patch` contains91 product files, including the exact retained C
engine. `boundary.patch` omits the vendor tree for review. `product-manifest.json`
checks every proposed product file against the qualified remote input. The full
1,355-file source inventories are identical before and after qualification:
`final-source-{before,after}.json`, source digest
`ba1c370fe413c9d21bf5ab3175e8008171a2846ddf2fbe41739b441588b3639b`.
Native binary SHA256:
`b8d982121e85d17e90bbf81629140b1637faa4f721cfad9efb2fa8525f22721a`.
It was built in the dedicated `/home/callen/.cache/hotaru-auto-links-target`;
the earlier shared-target stale-artifact failure is documented separately.

## Implemented behavior

The native window owns one matcher worker. Pointer requests retain the exact
immutable displayed snapshot, pane identity and source cell. The worker builds
a complete logical selection over soft wraps, stopping at each cell's semantic
content boundary, then uses the exact pinned Ghostty default regex and suffix
search algorithm. Explicit OSC8 presence takes priority, including invalid
explicit targets that native validation rejects. The public terminal core adds
only a read-only `Row::wrap_continuation` getter; the matcher is a private native
crate, independent of the core workspace.

A completed match is accepted only for the same request Arc, pane and displayed
snapshot. Press/release also require the same cell, target, modifier and pointer
position. Moving the pointer, replacing the request, closing a pane or displaying
a new frame invalidates old work. A click made before a match is available does
not become an automatic delayed activation. Existing OSC8 hover still clears on
a new displayed frame until pointer movement; automatic requests can refresh at
the stationary pointer. This distinction is covered by the old and new native
Wayland suites.

Automatic relative targets resolve lexically against the last accepted local
OSC7 directory. The initial launch directory is not substituted for an OSC7
report. As in pinned Surface.resolvePathForOpening, an existing resolved path
is opened, otherwise the original matched text is used. There is no shell
execution, variable/tilde expansion, canonicalization, or line-suffix stripping.
Resolution runs in a disposable instance of this same executable, entered before
GUI/instance initialization. Its supervisor admits one child, bounds output to
16KiB, starts a250ms deadline after spawn, kills its own child on timeout or
cancellation and retains ownership until it is reaped. Spawn and uninterruptible
kernel I/O have no hard deadline. The UI never joins the owner. Before invoking
the existing literal-argv xdg-open queue, request, displayed snapshot, active pane
and captured cwd are revalidated.

## Bounds and limits

Each matcher/resolver mailbox has one active request, one replaceable pending
request and one unread completion. UI messages carry only a notification, not
owned snapshots. Destruction of replaced payloads happens outside the mailbox
mutex. A matcher request is admitted only when the existing complete-frame
retention estimate is at most8MiB; three slots can therefore account for24MiB
per worker, plus bounded metadata and engine allocations. This is conservative
logical ownership, not RSS, and the UI has its own existing frame ownership.

Logical extraction accepts at most4,096 columns,4,096 visible rows,64KiB of text and65,536 mapping entries. Exceeding these caps is an unavailable match, not selection equivalence. It
never treats a truncated string as a complete regex input. A selection extending
beyond the visible snapshot returns no result. In particular, a selection
reaching visible row zero while earlier history exists is conservatively rejected,
even when the hidden predecessor might prove to be a hard line. Full scrollback
capture for automatic links remains a completeness gap. Configurable user regex
rules, always-on highlighting and automatic-link double-click selection are also
outside this slice; the Aizome modifier-click and existing focus-colored hover
presentation are retained.

The fixed regex is675bytes, SHA256
`3c80ce666b95e0ed44057e64c020dfd7f979df235bae7ade02af1ef83d0c30fd`.
The exact external Oniguruma6.9.9 C engine remains retained, not rewritten in
Rust. Its48 source units, recursive local headers and license have explicit
provenance and a checked owning Rust boundary. No Apple SDK or macOS platform
code was added. Existing cc1.8.0 builds it; no bindgen wrapper is used. Nix installs
the original BSD2 notice from `vendor/oniguruma/LICENSE`; Ghostty's existing MIT
notice covers the derived default rule.

A100,000-retry foreign search can exceed the nominal25ms cooperative deadline:
retained adversarial evidence includes about0.67seconds in one call. The worker
checks cancellation/deadline before and after each call, including errors, but
does not interrupt or join it. A hard matching deadline would require moving the
matcher itself into a killable process. Stack limits count entries, not bytes;
the default4,096-entry limit and512..16,384 accepted range include documented
initial-promotion and realloc peak caveats. No responsiveness or efficiency parity
claim follows from these bounds.

## Verification

- The frozen matcher prototype passed9debug and9release tests, strict Clippy,
  3,474 independently compiled exact-engine/suffix oracle cases, including all90
  pinned default-rule examples, plus44 bounded adversarial subprocess cases.
  Those historical source identities remain in `matcher/`.
- `mapping-oracle.json`:11,088 actual pinned Screen.selectLine/StringMap cases,
  all passed. The diagnostic clone is separate from the reference checkout;
  only a narrow exported adapter and its lib_vt export were added. It calls the
  actual methods with whitespace=null and semantic boundaries enabled. Tests
  cover one-column grids, UTF8, wide/combining/emoji, softwrap, blanks, erase,
  OSC133, explicit-link text and alternate screen. The gate compares logical
  selection bounds/text and clicked target ranges/endpoints. Ghostty emits some
  unprinted-blank PinMap coordinates backwards; those raw observations are saved.
  Rust uses physical forward blank positions. The fixed regex's matched endpoints
  do not lie on those blanks; this is not a claim of raw whole-PinMap equality.
- `final-tests.log`:73ordinary native tests passed (44library,2font,22native,
  5pixel-scroll); seven pre-existing intentional ignores remain. Strict package
  all-target Clippy passed. The added tests cover latest replacement, unread
  completion retention, no-join teardown, cancelled work, helper timeout/reaping,
  bounded output, literal path resolution, invalid explicit-link precedence and
  exact snapshot/pane/request activation.
- `wayland-qualified/`:real private Sway/pixman, exact Unicode URI argv, existing
  and missing relative paths, softwrap, semantic boundary, OSC8 priority, and a
  held gesture cancelled by a new frame. A final successful activation orders
  the negative case; the disposable opener never invokes a browser/network.
- `osc8-qualified/`:the existing real Wayland OSC8 suite also passes: wide tail,
  plain click/drag/stale frame/invalid UTF8 cancellation and visible stationary
  opener error. The pre-fix `osc8-regression/` failure and source inventory remain
  as red evidence for the narrowly corrected hover refresh.

Independent native review found no concrete ownership, stale-target or child
lifetime defect. Its suggested destruction-outside-lock cleanup is in the frozen
source. The regex FFI review separately found the original stack-unit mistake;
that superseded evidence is preserved, and the corrected API was re-reviewed.

## Reproduction and integration

Use the dedicated target and fonts recorded by the manifests. The native tests
and Wayland runners need the shared workstation flock. `mapping-oracle.py`
acquires that flock internally: never wrap it in a second same lock. Tooling and
exact reference identities accompany this archive; the matcher reference uses
Ghostty6ba144158577d033ccb6ed4b574e4013957c9161 and Zig0.17.0.

Apply the standard Git patch against8f05f4a6, preserving any subsequent core
margin/OSC52 changes via narrow hunks. The only core hunk exposes wrap continuation;
app.rs/main.rs changes must be reviewed alongside concurrent native work. The
standalone core Cargo workspace is unchanged. Desktop Cargo.lock, private crate,
Nix license notices and smoke source inventory are included. Shared product and
package qualification remain the parent's integration task.
