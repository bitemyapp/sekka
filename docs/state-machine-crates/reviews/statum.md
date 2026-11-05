# Review: impl-statum (statum =0.9.0)

Reviewer checks run: `cargo test --locked -p impl-statum` (all 12 spec tests and
all 36 trybuild cases pass, target dir `sme-review-statum`), plus two scratch
crates outside the workspace, not kept. Neither scratch crate touches workspace files:
`statum-probe` tests sealed payloads and the E0119 collision, and
`statum-forge` tests the unit-edge forge cases proposed below.
Cases are classified under the new "Transitions and forging" rule
(IMPLEMENTING.md:227-269) and under the clarification "In a typed API a
transition stands for its event" (IMPLEMENTING.md:274).

## 0. The central finding, verified

**The claim holds: statum generates public, guard-free edges.**

- **Code that emits them.** `statum-macros-0.9.0/src/transition/emit.rs:80-181`
  loops over every target state that a `#[transition]` method's return type
  names. For each one it emits an
  `impl statum::CanTransitionWith<Data>` and an `impl statum::CanTransitionMap<Next>`
  (`:115`, `:145`; data targets) or an `impl statum::CanTransitionTo<Next>`
  (`:170`; unit targets).
- **Inherent `transition_map`.** `machine/emission/support.rs:332-341` adds an
  inherent `#vis fn transition_map` on the machine itself, with the machine's
  own visibility.
- **No gate.** No `cfg`, attribute or feature controls any of this; only the
  introspection registrations are behind a feature.
- **Effect.** Each impl moves the machine's fields into the new state
  (`emit.rs:107-111`, `:131-141`) without running the method body.
- **Private methods leak too.** The impls are emitted even when the
  `#[transition]` method is private. Examples are `install.rs:160,165` (the
  private `build` and `deferred` give I3/I4 their edge), `pointer.rs:163`,
  `transfer.rs:138,142` and `lock.rs:80`.
- **The traits are a documented public API.** `CanTransition*` are re-exported
  "advanced traits" (`statum-0.9.0/README.md:15`, `src/lib.rs:304`), "the
  stable trait-level view of `self.transition()`"
  (`statum-core-0.9.0/src/lib.rs:89-128`).
- **The private helper traits are unreachable.** The support traits
  `TransitionTo/With` live in a private `#[doc(hidden)] mod`
  (`support.rs:161-172`), so outside crates can't call `.transition()` or
  `.transition_with()` directly. They can call the public ones.

**What the 12 accept cases use.** They use only items a downstream crate can
reach:

- `impl_statum` types;
- generated `pub` `XData` structs with `pub` fields (`state/emission.rs:67-77`);
- the `pub state_data` field (`support.rs:328`, always `pub`);
- the inherent `transition_map`;
- `statum::{CanTransitionTo, CanTransitionWith}`.

None of these is `#[doc(hidden)]` or under `__private`. The cases are
legitimate.

**Can an implementation hide them?** This is the coordinator's central
question. The answer is: partly, and the implementation hides less than
statum allows.

- **No statum option or attribute hides them.** statum's documented boundary is
  the machine's own visibility. The v0.9.0 docs
  (`docs/generated-builder-reference.md`, section "Visibility") say: "Generated
  builders follow the machine struct's visibility". `docs/escape-hatches.md`
  doesn't list `CanTransition*`, `transition_map` or the builders as bypasses:
  statum treats a declared edge as legal for anyone who can name the machine.
  A private machine also hides the typed API, as NOTES says.
- **Builders: hidden by the `Seal` field.** The workaround is sound (detail in
  §3).
- **Data-target edges and data writes: hideable, but not hidden here.** statum
  accepts tuple variants with any payload type (`InReview(Assignment)`,
  `lib.rs:322-345`). If each data state's payload is a hand-written struct with
  private fields:
  - an outside crate can't build the payload for `transition_with_data`, or
    return one from a `transition_map` closure. Verified in
    `statum-probe/tests/outside.rs`: both fail with `E0451: field deferred of
    struct PreparedTok is private`.
  - outside crates also can't write the payload's fields.

  This is the same mechanism as typestate's I4 reject (`E0451`) and as this
  slot's own `Seal`. The implementation instead used named variants, whose
  generated `XData` fields are always `pub`. Rule 1 ("A means your
  implementation chose to expose … hide it") applies.
- **Unit-target edges: not hideable.** `CanTransitionTo<Unit>` stays callable,
  verified: `bypass_unit` compiles. The exception is giving the "unit" state a
  sealed token payload. Verified: `Copy(CopyTok)` gives `E0423: cannot
  initialize a tuple struct which contains private fields`. That turns every
  unit state into a data state, so it is a real cost. The implementer should
  decide whether to do it and record the decision.
- **The `pub state_data` field itself: not hideable.** It can only be swapped
  with another value of the same, sealed type. Tokens must not be `Copy` or
  `Clone`, or a caller can harvest one from a legitimately reached state and
  reuse it. One example is a copied `RetryWait` payload replaying K3.

**E0119 claim: verified.** `TransitionWith<#data_ty>` is keyed by payload type,
and `NextState` is an associated type (`emit.rs:104-105`, `support.rs:114`). Two
targets of one source state with the same payload type collide. Reproduced:
`collide` feature, `error[E0119]: conflicting implementations of trait
CanTransitionWith<ItemId> for type collide::Job<Asking>`. The dedup
`emitted_states` is per block (`emit.rs:74-83`), so one edge declared in two
blocks also collides, as NOTES says.

**Source-reading and global-registry claim: verified, but the "maintenance
risk" framing is overstated.** What the code does:

- reads files with `fs::read_to_string` (`source/parser/mod.rs:45`,
  `source/analysis/mod.rs:18`), from `Span::local_file()`
  (`source/module_path.rs:29`);
- keeps process-global registries (`machine/registry.rs:6`,
  `state/registry.rs:6`, `source/cache.rs:37-38`), invalidated by
  file length and mtime (`source/syntax.rs:70-82`);
- switches builder emission under `RUST_ANALYZER_INTERNALS`
  (`machine/metadata.rs:250-252`, `machine/emission/builders.rs:37`).

How much that matters:

- **rustc and reproducible builds: little.** The files read are the crate's own
  module files, which rustc already tracks, and proc macros re-run on every
  build of the crate. In the default feature set no path reaches the output.
  With `introspection`, the hashed file path becomes symbol names
  (`machine/introspection.rs:26-36`), which is path-dependent.
- **Real risks:**
  - rust-analyzer sees different generated code;
  - the long-lived RA proc-macro server relies on an mtime cache;
  - items not on disk degrade (`include!`, code generated by other macros).
    The implementation's `macro_rules!` blocks work.

## 1. Idiom fidelity, per target

| target | grade | what the DSL holds | hand code beside it |
|---|---|---|---|
| pointer | partial | Six `#[state]` leaves with data. Every row is a `#[transition]` method (`pointer.rs:105-336`). | `inside!` for `Inside`'s exits (`:117-132`). The 3-way `press` returns a hand-written `Pressing` enum built from declared edges (`:134-182`). `handle` adds an enter-first pre-step (`:366-369`). All recorded. About 95% of rows are in the DSL. |
| wrap | idiomatic | Four unit states. Every row is in four `#[transition]` blocks (`wrap.rs:170-289`). `Branch` aliases are followed. | The cursor helpers are a generic plain impl (`:90-168`): context code, the same as typestate. `handle` classifies controls (`:351-364`). About 100% of rows are in the DSL. |
| polkit | partial | Phases with data. Each state's edges are in one block (`polkit.rs:100-261`). | `conversing!` for `Active` (`:124-161`); recorded. The stale-conversation filter and the Begin/Cancel/Dismiss routing are in `handle` (`:334-359`); these runtime guards match typestate. PamError/PamInfo notes are pushed in `handle` (`:362-369`), although `conversing!` defines a `note` method that is never called (`:155-159`). About 85% of rows are in the DSL. |
| transfer | partial | Per-item states with data, one block each (`transfer.rs:120-281`). | The 4–5-way `Probed` enum (`:129-173`). `next`/`done`/`cancel` are generic functions bounded on `CanTransition*` (`:63-106`), which is a legitimate use of statum's traits. Frames are a `Vec` field, as in both baselines. The item guards are in `handle`. About 90% of rows are in the DSL. |
| lock | partial | Two machines (regions). Lock leaves with data, delay leaves unit (`lock.rs:18-253`). | Cross-region transitions take `&mut inhibitor::SomeState` (`:87-93`, `:172-186`). The delay-event routing and the PrepareForSleep decision are written in `handle` (`:284-298`, `:350-361`); that is the region coordination, recorded. `typed`/`tick`/`output` are plain methods. About 75% of rows are in the DSL. |
| install | partial | 20 states, one block per stage (`install.rs:81-330`). | `preparing!`/`executing!` (`:98-133`) and the `Route` enum (`:152-196`); recorded. `handle` is pure dispatch. About 100% of rows are in the DSL. |

No target is a facade: every state change goes through a `#[transition]`
method. No target is under-used: statum has no hierarchy, regions, events,
entry/exit or timers that could have been used. Its `#[validators]` and
`introspection` don't apply here.

## 2. Compile-fail audit

Under the new rule, all 12 `accept` cases went through only by forging. Each
moves to the class the transition API gives it (`#[transition]` methods and
`handle`). In every case the first error below is the guarantee: wrong state
type, no such method or field.

| case | new class | transition-API attempt | expected first error |
|---|---|---|---|
| W1 | reject | `screen::SomeState::Pending(normal)`, as typestate's W1 (`Any::Pending(normal)`) | E0308: `Screen<Pending>` expected, `Screen<Normal>` found |
| K1 | reject | `Session::<Prompted>::answer(starting, &mut out)` | E0308 (or `starting.answer(..)`: E0599 with "found for `Session<Prompted>`") |
| K3 | reject | `Session::<RetryWait>::retry(starting, &mut out)` (restart without the back-off) | E0308 |
| T3 | reject | `restoring.skip(&mut out)` (a real method of the asking states) | E0599 with "found for `Job<AskingFiles>`…" |
| T4 | reject | `Job::<SettingAside>::set_aside(asking, AsideId(9), &mut out)` (place over the target without an answer) | E0308. The implementer's `asking.place_at(..)` names no existing method; prefer a real one. |
| T6 | reject | `asking_files.merge(&mut out)` | E0599, "found for `Job<AskingFolders>`" |
| L3 | reject | `locking.submit(&mut out)` | E0599, "found for `LockScreen<Locked>`" |
| L6 | reject | `locking.tick(&mut out)`, or the data-shape form `locking.state_data.shake` | E0599, or E0609 "no field `shake` on type `()`" |
| I1 | reject | `prepared.erase(&mut out)`, or `Helper::<Erase>::done(prepared, ..)` | E0599 / E0308 |
| I2 | reject | `Helper::<Copy>::done(files, &mut out)` (bootloader from `Files`) | E0308 |
| I3 | reject | `evaluation.confirm(true, &mut out)` | E0599, "found for `Helper<Prepared>`" |
| I4 | reject | Under "a transition stands for its event", every typed path to a deferred `Prepared` goes through `evaluated`, which demands the Evaluated event's sizes and runs the check (`install.rs:165-195`). The only other source is the private `deferred`, and the builder is sealed. Attempt: `Route::Deferred(evaluation)`, which treats the unchecked evaluation as the route's `Prepared`. | E0308: `Helper<Prepared>` expected, `Helper<Evaluation>` found |

**Resulting tally: 31 reject, 5 runtime, 0 accept.** This is the same as
typestate (31/5/0), and the per-probe classes are identical. The typed-API
clarification below may move L1 in both slots.

**Typed-API clarification ("a transition stands for its event") applied to
both slots.** I checked every "X without E" probe in statum and in
impl-typestate:

- **Unchanged.** P1, W2, W4, K1, K3, K6, T1, T4, I1, I2 and I3 stay `reject`
  in both. Every typed path to X goes through a transition named for E:
  `press`/`capture`, `prompt`, `retry` (timer), `choose`, `placed`, `replace`
  or `keep_both`, `confirm`, step 4's `done`.
- **I4 (classification changed).** Previously I classified I4 `runtime`; it is
  `reject` (table above), because `evaluated` demands E's result. Typestate's
  I4 class is therefore right. Its attempt, a private-field variant literal,
  only shows that the forging means is hidden. A transition-shaped attempt
  such as `Route::Deferred(evaluation)` would show the guarantee more
  directly (should).
- **L1 (flag, both slots): probably no longer `reject`.**
  - **The escape.** Both slots have a public `release` on the held delay:
    `Inhibitor<Held>::release` (`impl-statum/src/lock.rs:58`) and
    `DelayFd::release` (`impl-typestate/src/lock.rs:30`). Neither is named for
    the lock's confirmation, and neither demands a proof of it.
  - **How L1 is committed.** With the screen still `Locking`, a caller can
    answer PrepareForSleep with `fd.release(out)` instead of
    `fd.hold_for_sleep()`. That releases the delay for a pending sleep before
    the lock is confirmed.
  - **Why the current reject misses it.** The case only shows that
    `SleepPending` has no `release`.
  - **Fix, fair game under the clarification.** Make `release` demand the
    lock region's proof, for example `release(self, _: &LockScreen<Locked>, out)`,
    or `&Locked` in typestate. Or move it into the `Locked`/`Checking`
    transitions. Record the lines.
  - **Without that fix, L1 is `accept` in both slots.** Either way the
    statum-versus-typestate comparison is unchanged.
- **L7 stays `reject`.** `lost` stands for LogindVanished.
- **Runtime cases stay `runtime` in both:** K2, T5, L2 (stale ids), L4 and L5
  (`refused` takes any note; `handle` maps it).

**Forge cases** (`cases/statum/<target>/forge/F-<means>.rs`). One is needed per
target where a means stays public because statum generates it. With the
current, unsealed code, every target has one:

| target | case | uses | survives sealing (§6 fix 3)? |
|---|---|---|---|
| pointer | `F-can-transition-to.rs` | P5: `CanTransitionTo::<Outside>::transition_to(hovering)` after a HoverOn. Asserts a HoverOn with no HoverOff, state `Outside`. Verified in `statum-forge/src/main.rs`. Other means for the comment: `CanTransitionWith<EdgePressedData>` (forged edge press with any `beneath`), `pub state_data` writes (`EdgePressedData.at`: P3). | yes |
| wrap | `F-can-transition-to.rs` | W1: the current accept body | yes |
| polkit | `F-transition-with.rs` | K3: the current accept body; comment: K1 via `transition_map` | **no**. Sealed, only `→ Idle` unit edges remain, and they commit no K-probe. Then write no case and say so in NOTES §4. |
| transfer | `F-can-transition-to.rs` | T3: `CanTransitionTo::<Finished>::transition_to(restoring)` (the edge `restored`'s `Next` declares). The aside is dropped and nothing is reported. Verified. Comment: T4/T6 via `transition_map`/`CanTransitionWith`. | yes |
| lock | `F-transition-with.rs` | L3: the current accept body; comment: L6 | **no**. Sealed, the remaining unit edges either equal action-less methods (`lost`, `resumed`, `hold_for_sleep`) or skip actions that are not in L1–L7. Write no case; NOTES §4. |
| install | `F-can-transition-to.rs` | I2: the current accept body. Comment: I1/I3/I4 via `transition_map`/`CanTransitionWith` while payloads are unsealed. | yes (unless `Copy`/`BuildOnTarget` get tokens) |

**Existing reject and runtime cases.** I agree with all of them. Notes:

- **W3/W4/L1/L7/I5/K4–K6.** Their comments argue that a generated edge "adds
  nothing". Under the new rule that argument is unnecessary. It is harmless,
  but replace it with the rule.
- **W5.** The first error is E0624 (private `soft_wrap`) where typestate gets
  E0599. The private helper is the guarantee, so it is acceptable.
- **Runtime cases.** K2, L2, L4, L5 and T5 stay runtime (rule 4: forging would
  defeat them). They read `state_data.*` from outside, so after sealing they
  need getters.
- **A design hole shared with typestate.** `Inhibitor::<SleepPending>::resumed(p).release(out)`
  while the screen is `Locking` releases a delay through public transitions.
  It is only "legal" if `resumed` is taken as the Resumed event. This is not
  statum-specific and I am not proposing a change.

## 3. NOTES.md claims checked

- **Correct:**
  - public builders with the machine's visibility and no option to hide them
    (`builders.rs:34`, `builder/typestate.rs:245`);
  - the `Seal` stops builders: `Seal(())` has a private field, the machine's
    `seal` field stays private (`support.rs:278-284` keeps the field
    visibility), and nothing harvests it;
  - E0119 by payload type and by duplicate block;
  - `pub state_data` (`support.rs:328`);
  - named-variant `XData` with `pub` fields (`state/emission.rs:67`);
  - `SomeState` comes from `#[machine]` without `#[validators]` (`lib.rs:361-365`);
  - MSRV 1.93; `statum` is edition 2021 (`statum-macros` is 2024);
  - `statum-macros` is 13,258 lines;
  - `span-locations` (`statum-macros-0.9.0/Cargo.toml:58`);
  - README says `0.8.10`;
  - source reading and the global registry.
- **Overstated:**
  - §4/§9: "statum has no switch to turn that surface off. Containing it means
    keeping the machine types private to their module." There is no switch,
    but private payload types neutralise every data-target edge and every data
    write. Only unit-target `CanTransitionTo` remains, and per-state tokens can
    close that too (§0).
  - §4: "I counted a bypass as an accept only when no sequence of hand-written
    transitions produces the same effects" is superseded by the forging rule.
  - "12 of 36 probes accepted against 0" in §9 and the survey corrections is
    superseded too.
  - §7: "a maintenance risk for incremental builds" is not supported for rustc
    (see §0). Keep the rust-analyzer and the not-on-disk points.
- **Not checkable without benches:** the smoke ns/event figures. The lock gap
  is discussed in §5.

## 4. Test integrity

- **Harness files.** `tests/spec.rs` and `tests/compile_fail.rs` differ from
  impl-typestate, statig, smlang and finny only in the crate name. impl-enum
  differs by design: no differential tests, plus coverage tests.
- **`NOT_EXPRESSED` and ignores.** `NOT_EXPRESSED` is empty and there are no
  `#[ignore]`s. Conformance passes for all six targets and the differential
  tests pass (reviewer run).

## 5. Benchmark distortions

There is no per-event allocation, `dyn` dispatch, queue or `Err`. Each event
costs one `Option::take().expect()` branch, documented as the alternative to a
builder placeholder. Two remarks:

- **`lock` delay placeholder.** `lock` still builds a builder-made placeholder
  `D::NotHeld(Inhibitor::new())` for every `mem::replace` of the delay: on
  each delay event (`lock.rs:286`), on PrepareForSleep (`:355`) and in
  `settle` (`:89`). The other `SomeState`s use `Option`. Use `Option` plus
  `take` for `delay` too, then re-measure replay-lock. It is the target with
  the unexplained 889 vs 509 ns gap.
- **No `#[inline]` on generated code.** The generated `transition()` and
  builder setters carry no `#[inline]`, and the bench profile has no LTO
  (workspace `Cargo.toml:45-49`). Cross-crate inlining into the bench crate's
  monomorphised `handle` depends on rustc's automatic small-leaf inlining.
  That is a crate cost, not a distortion; phase 3 should keep it in mind if
  the lock gap persists.

## 6. Required fixes

1. **must.** Reclassify all 12 accepts as reject, as in the §2 table. Write
   12 reject cases (W1, K1, K3, T3, T4, T6, L3, L6, I1, I2, I3, I4), each
   with its `.stderr`, and confirm that each first error is the one listed.
   For I4 the attempt is `Route::Deferred(evaluation)`, as the "transition
   stands for its event" clarification requires. Delete the `accept/` files.
   Separately, the L1 flag in §2 applies to both slots, and its fix is a
   coordinator call. Either make `Inhibitor<Held>::release` demand a
   `&LockScreen<Locked>` proof, or move L1 to `accept`.
2. **must.** Add one forge case per target, as in the §2 forge table:
   - pointer: P5 via `CanTransitionTo<Outside>` (verified);
   - wrap: W1;
   - transfer: T3 via `CanTransitionTo<Finished>` (verified);
   - install: I2;
   - polkit (K3) and lock (L3): only if fix 3 is not done.

   In each case's comment, name the other public means: `transition_map`,
   `CanTransitionWith`, and `pub state_data` writes.
3. **must** (rule 1). Hide what statum lets you hide. Replace each data
   state's named variant with a tuple variant holding a hand-written per-state
   payload struct with private fields, plus getters where `handle` or the
   cases read it. Examples: `Prepared(Prepared_)` or `Prepared(Route)`; the
   naming is up to you. Requirements:
   - Payload types must be distinct per state, so that no source state shares
     a payload type with a target. This also avoids E0119.
   - Payload types must be neither `Copy` nor `Clone`, so they can't be
     harvested and replayed.
   - Recheck the data-shape rejects (P2–P4, I6, L6), whose first errors become
     "private field" / "no field" / "no method".
   - Drop the polkit and lock forge cases and say why in NOTES §4.
   - Either give unit states sealed tokens too and drop every forge case
     (verified to work: E0423), or record why not and what it would cost.
   - Report the LoC delta.
4. **must.** Rewrite NOTES §4 (tally, the forge list, "hiding": builders by
   `Seal`, data edges by private payloads, unit edges not hideable without
   tokens, `pub state_data` not hideable), §9 and the survey corrections to
   match. Cite statum's own boundary statement
   (`docs/generated-builder-reference.md` "Visibility") and the fact that
   private `#[transition]` methods still emit public impls.
5. **should.** NOTES §7: replace "maintenance risk for incremental builds"
   with the accurate risks: rust-analyzer divergence, the mtime cache in the
   long-lived RA server, items not on disk, and path-hashed symbols under
   `introspection`.
6. **should.** Make `lock`'s `delay` an `Option` with `take` (`lock.rs:89`,
   `:286`, `:355`), then re-run smoke and `replay-lock/statum`.
7. **should.** In polkit, have `handle` call the `note` method from
   `conversing!` for PamError/PamInfo (`polkit.rs:362-369`), or delete the
   method (`:155-159`).
8. **should.** Drop the reject comments' "generated edge adds nothing"
   arguments (W3, W4, L1, L7) in favour of the rule.

## 7. Overall

The implementation is faithful: every state change is a `#[transition]`
method, the workarounds are honest and recorded, there are no distortions
beyond `lock`'s placeholder, and the tests are intact. The central finding is
real: statum publicly generates guard-free edges, including for private
transition methods, and offers no option to stop it. Under the new rules,
though, it no longer decides the probe classes. The transition API rejects
exactly what typestate rejects (31/5/0), and the L1 flag affects both slots
alike. The fair comparison point is what can be hidden.
The implementation hid the builders but left the data edges and data writes
open, although private payload types close them. As shipped, it therefore
overstates statum's exposure. Once fixes 1–4 are done, statum can be ranked
on typestate parity for classes, with forge entries only for unit-target
edges (pointer, wrap, transfer, install) and a measured LoC cost for sealing.
