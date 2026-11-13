# Review: impl-statig (statig =0.4.1, statig_macro 0.4.0)

Reviewer checks run (own target dirs, no workspace edits):

- `cargo test --locked -p impl-statig`: 12/12 spec tests and the trybuild run
  pass (traces 21/14/16/20/18/16, all of them).
- `SME_PROPTEST_CASES=5000 cargo test --release --test spec`: 6/6
  differentials pass.
- `cargo fmt --check` is clean. `sme.py loc` gives 1630 against enum's 1165,
  as NOTES says. The smoke rows all say `same as enum`. `sizes` matches NOTES.
- Experiments in a scratch copy of the workspace
  (`exp/sme`, not kept): variants of polkit, wrap and lock, callgrind
  instruction counts, and a standalone statig crate (`exp/two`, not kept)
  for the hook, E0252 and `state_mut` claims.

## 1. Idiom fidelity per target

| Target | What statig holds | Hand code beside it | Rows in statig | Grade |
|---|---|---|---|---|
| pointer | 6 leaves; `inside` superstate (`pointer.rs:121`) owns the hover as borrowed state-local data; its exit action clears the hover. Every row is a handler arm returning `Handled`, `Super` or `Transition`. | `outside` (`pointer.rs:92-111`) repeats hovering's Move/Press/Release (through a shared `first_press`, `:64`), because statig can't re-dispatch. `Hover::track` is a plain helper (an action body). | ~100% (3 rows duplicated) | idiomatic |
| wrap | 4 data-less leaves under the `autowrap` and `no_autowrap` superstates (`wrap.rs:126`, `:175`), matching the spec's composites. Leaves handle Print, SetAutowrap and SaveCursor. Superstates handle the controls and FullReset. | `Screen::control` (`:83`) returns `Option<bool>` and the superstate maps it to a leaf. `Repeat(n)` is unrolled in `handle` (`:248`). | ~95% (Repeat) | idiomatic |
| polkit | `idle`; `active` (open/close-dialog entry and exit) ⊃ `conversing` (end-helper exit) ⊃ {starting (entry = start), prompted, checking}; `retry_wait` (arm/cancel), `unavailable` (give-up entry). Conversation and request are state-local data, and the guards are on them. | Three flags for event-dependent exits (`helper` `:182`, `armed` `:237`, `authorized` `:146`). A hand `EndHelper` for restarts inside `conversing` (`:152`, `:225`). `Cx::again` re-dispatch for `Begin` with another cookie (`:103`, `:290`). | ~90% | partial (honest, recorded; slightly under-used, see fix 1) |
| transfer | One leaf per awaited response with state-local item and aside. Every request is an entry action (`transfer.rs:94-248`), so the self-transition probing → probing re-probes. | The merge-frame stack is shared storage (`Vec`), with `next`/`done`/`cancel` helpers that return `Outcome` (`:44-78`), the same as enum. statig's hierarchy is static, so recursion can't be a superstate. | ~85% | partial (honest, recorded) |
| lock | Lock region: `lockscreen` ⊃ {locking, `locked` ⊃ {idle, checking}}, with all the spec's entry and exit actions. Delay region: a second statig machine (`holding` superstate, entry = TakeDelay) in submodule `delay`. | A coordinator (`Lock::handle`, `lock.rs:339`) dispatches every event to both machines. The cross-region guards read the lock phase through `delay::Cx` (`:273`, `:293`). The `release` flag works around `LockEnded`. | ~85% | partial (honest, recorded) |
| install | `preparing` and `executing` superstates (StayAwake/AllowSleep as entry and exit, `install.rs:174`). Every stage runs from its entry action, and every terminal state reports from its entry action, so the order is produced by the hierarchy. | none | 100% | idiomatic |

No target is a facade. Where hand code exists, it decides data, not which
state comes next, except for the lock coordinator and polkit's `again`, which
are recorded workarounds.

## 2. Compile-fail audit

I found no misclassified case. The table gives 7 R, 29 T and 0 A, matching
NOTES. Every reject's first error is the guarantee: a field missing from, or
required by, a generated variant (E0026/E0559/E0063). All reject cases use
variant literals. All runtime cases assert the refusal (no effect, unchanged
state). All `.stderr` files match: trybuild passes. P3's message has no path
prefix, which is rustc's choice and harmless.

Notes for phase 3, with no file changes needed:

- **L6 (R) is data shape only.** `State::Locking` has no `shake`, but a
  `locking` handler may still push `ArmShake`. That puts it on the same footing
  as enum's L6 R; it is not a timer protocol.
- **K5 is consistent as T.** By L6's logic it could be R
  (`State::Starting { .., armed: true }` gives E0559), but `armed` is a
  workaround flag that disappears under fix 1.
- **The 0 accepts come from the wrapper, not the crate.** They hold because
  every storage type has private fields and the `machine` field is private.
  statig itself lets safe code start a machine in any state: I reproduced
  `*m.state_mut() = State::Q {}; m.init()`, which runs the superstate's and
  then Q's entry actions. Put this in the tally (fix 5) so the ranking doesn't
  credit statig with 0 accepts.

## 3. NOTES.md claims, checked

| Claim | Verdict |
|---|---|
| `before_transition` gets source and target swapped (`blocking/inner.rs:42` passes `(&target, &self.state)` to the `(source, target)` hook at `into_state_machine.rs:241`); `after_transition` is right (`:58`, after the swap) | **Correct.** Reproduced: p→q gives `before(source=q, target=p)` and `after(p, q)`. |
| A handler can't forward the event to its new state, so polkit dispatches `Begin` twice | **Correct.** `Outcome` has only Handled/Super/Transition (`outcome.rs:182`). `Inner::handle_with_context` dispatches once (`inner.rs:27-38`). Handlers can't reach the machine. The second dispatch happens for 2 of the 94 replay events. |
| Superstate exit and entry are decided by variant only, never by data | **Correct.** Leaves: `same_state` compares `discriminant` (`state.rs:40-42`), and an equal leaf gives `(1,1)` (`:68-70`). Superstates: transmuted discriminants (`superstate.rs:44-55`) inside `common_ancestor_depth`. Starting(A)→Starting(B) runs only starting's own exit and entry. |
| Two `#[state_machine]`s in one module give E0252 (`codegen.rs:21` emits `use statig::{state, superstate, action};`) | **Correct.** Reproduced: E0252 for all three names, even with `state(name=…)` and `superstate(name=…)` renaming both enums. |
| `UninitializedStateMachine::state_mut` is safe (`state_machine.rs:729`); the initialized one is `unsafe` (`:475`) | **Correct.** The lazy `StateMachine::state_mut` (`:201`) is also unsafe. `init` enters every level of whatever state was set (`inner.rs:20-24`). Reproduced. |
| The State enum is `pub` by default, its constructors are private, and `#[superstate(initial)]` gives a `Superstate::name()` | **Correct.** `analyze.rs:207`; `lower.rs:540` (`fn` without `pub`); `codegen.rs:392-409`. |
| `#[default]` fields are left out of the constructor | **Correct.** `lower.rs:510-528` |
| MSRV 1.66, README badge 1.65 | **Correct.** |
| Ignored events cost nothing extra (no `Err`) | **Correct.** |
| Each leaf must declare every field of its superstate chain | **Correct for leaves** (`lower.rs:216`: the leaf arm builds `Some(Superstate::X { fields })` from its own bindings). **Overstated for intermediate superstates**: `#[superstate(local_storage("release: &'sub mut bool"))]` lets `locked` drop the unused parameter. Validated: lock 18/18 plus differential, no warnings. |
| "Exit actions can't see the event or the target, so … flags" (§2 polkit and lock, §6, §9 "fought me in four places") | **Overstated.** True of the action signature. But the context is the user's own type and reaches every action, and `handle` can put the `Copy` event in it. Validated in the scratch copy: `Cx { out, again, event }` removes `Request::authorized`, `Conversation::helper` and `retry_wait.armed`, and polkit still passes 16/16 traces plus the differential. Lock's `release` would go the same way (`!matches!(event, LockEnded)`). |
| polkit costs ≈3.4× enum on replay | **Correct, and it comes from the crate.** See §5. |
| "proc-macro-error2 is flagged unmaintained"; "the hook fix has been unreleased since November 2025" | **Not verifiable offline.** The only local evidence is cargo's future-incompat warning. RUSTSEC-2024-0370 concerns `proc-macro-error` (v1). Cite sources or soften. |

## 4. Test integrity

- `tests/spec.rs` and `tests/compile_fail.rs` are byte-identical to every
  other crate slot's after the crate name is substituted. Both are unchanged
  since the phase-1 commit (`git diff ae3612e` is empty).
- `NOT_EXPRESSED` is empty, and there are no `#[ignore]`s. That is
  justified: everything passes, including 5000 release cases.

## 5. Benchmark distortions

Polkit replay, callgrind Ir per replay of all 16 traces (94 events):

| Variant | Ir/replay | vs enum |
|---|---|---|
| impl-enum | 10,105 | 1.00 |
| impl-statig as is | 34,203 | 3.38 |
| without the `again` re-dispatch | 33,180 | 3.28 (re-dispatch ≈ 3% of statig's cost) |
| effects buffered in `Cx`, no `dyn` | 34,604 | 3.42 (buffering is slower) |

Inclusive shares of statig's replay:

- `StateExt::transition_path`: 26%. It calls `depth()` and
  `common_ancestor_depth()` recursively, rebuilding borrowed `Superstate`
  values at every level, O(depth²).
- Superstate bubbling: 9%.
- Superstate exit and enter traversal: 9%.
- The out-of-line `Digest::push`: 8%.

Polkit has the deepest hierarchy (three levels) and transitions on most
events, so the 3.4× is the crate's per-transition machinery. Neither the
double dispatch nor the flags cause it.

Wrap (20k events at 80 columns): statig is 1.83× enum. A per-call buffer
costs +17% and a persistent buffer +11% over `dyn`, so the `dyn Sink` is not
a distortion.

Per event, the implementation adds no allocation (transfer's `Vec` grows only
on `Start`/`Listed`, as in enum), no `Err`, no queue and no clone beyond small
`Copy` state data. Two costs are inherent workarounds: lock dispatches twice,
and wrap's `Repeat` is n dispatches (enum also loops n prints). Wrap's
settling self-transitions (normal→normal) don't show in the profile.

## 6. Required fixes

None is a *must*: no conformance, classification or benchmark result
changes.

1. *should*: Pass the event through the context (`Cx { out, again, event }`;
   a lock `Cx` with `event`). Then drop `Request::authorized`,
   `Conversation::helper`, `retry_wait`'s `armed` (`polkit.rs:46,54,234-255`)
   and `lockscreen`'s `release` (`lock.rs:83-89,120-125,131,178,205,214`), and
   rerun loc and sizes. If flags are still preferred, say why. In any case,
   reword NOTES §2/§6/§9: actions are blind to the event *unless it is routed
   through the context*.
2. *should*: In NOTES §3/§9, attribute polkit's 3.4× to statig's transition
   machinery using the numbers in §5 (re-dispatch ≈3%; a buffer is slower than
   `dyn`), so phase 3 doesn't discount it as an artifact of the implementation.
3. *should*: At `lock.rs:140-142`, give `locked` the `release` field through
   `local_storage("release: &'sub mut bool")` instead of an unused parameter
   and `#[allow(unused_variables)]`. Narrow NOTES §2 "Each leaf must
   declare…" to leaves.
4. *should*: In NOTES §9 (maintenance risk), cite the advisory for
   proc-macro-error2 and the upstream commit and date of the hook fix, or
   reduce both to what is observable (the future-incompat warning; the bug in
   0.4.1).
5. *should*: In NOTES §4's tally line, say that the 0 accepts rest on private
   storage fields and the private `machine` field. statig's safe
   `UninitializedStateMachine::state_mut` plus `init` would accept I1, T3, W1
   and similar for any storage that outside code can construct.

## 7. Overall

This implementation represents statig fairly and can be ranked as it stands.

- Every target uses statig's real mechanisms: state-local data, superstates,
  and entry and exit actions in the spec's order. No `enum` facade hides
  behind it.
- The workarounds are real limits of 0.4.1 and are recorded honestly:
  - no regions;
  - no re-dispatch;
  - least common ancestor by discriminant only;
  - static hierarchy.
- The crate claims hold against the source and in reproductions: the swapped
  hook, E0252, the safe `state_mut`, and the discriminant-only least common
  ancestor.
- The costs measured belong to the crate, not to the implementation.
- The one place the implementation is a little harsh on statig: the
  "exit actions blind to the event" flags could mostly be replaced by carrying
  the event in the user-defined context.
- The 0 accepts reflect the wrapper's privacy more than statig's guarantees.
