# Review brief (phase 3a): independent check of crate implementations

Eight agents each implemented six of Sekka's state machines with one Rust
state-machine crate, in the workspace `../eval/`. The results
will rank the crates by expected value: performance, correctness,
expressiveness, concision, maintenance risk. A ranking is only as good as the
fidelity of each implementation, so your job is to find where an
implementation misrepresents its crate, in either direction:

- **Too generous to the crate**: a hand-written `enum` + `match` (or plain
  struct fields) doing the real work behind a thin crate facade, so the
  crate gets credit for passing traces it did not express; compile-fail
  "reject" cases whose first error is a typo, an import mistake or a private
  constructor rather than the guarantee; `runtime`/`accept` cases that don't
  assert what they claim; weakened tests.
- **Too harsh on the crate**: a workaround or "not expressible" claim where
  the crate actually has a feature for it (check the crate source and docs
  in `~/.cargo/registry/src/*/<crate>-<version>/`); a probe classified
  `accept` that the crate's typed API would reject if used as intended;
  avoidable allocation or cloning that would distort the benchmarks.

Read first: `../eval/IMPLEMENTING.md` (the contract the
implementers followed), each target's `spec/src/<target>/SPEC.md`, and
`impl-enum/src/<target>.rs` (the reference). Then for each crate you review:
`impl-<slot>/src/`, `impl-<slot>/tests/`, `impl-<slot>/NOTES.md`,
`compile-fail/cases/<slot>/`, and the crate's own source.

Do not edit any file in the workspace. You may build and run tests through
the wrapper with your own target dir:
`CARGO_TARGET_DIR=<scratch>/t/sme-review-<slot> ../eval/isolate default <scratch> bash -c 'cd docs/state-machine-crates/eval && CARGO_BUILD_JOBS=4 cargo test --locked -p impl-<slot>'`.
Python inside the wrapper: `python3`, from the root flake's default dev shell.
`python3 metrics/sme.py compile-fail` prints the classification table.

For each crate, write
`../reviews/<slot>.md`
with:

1. **Idiom fidelity, per target**: what the crate's DSL actually holds
   (states, transitions, guards, actions, entry/exit, hierarchy, state data)
   versus what is hand code beside it. Give a rough share of the spec's
   transition rows expressed in the crate's mechanism, with file:line
   evidence. Grade each target: *idiomatic*, *partial* (honest workaround,
   recorded in NOTES), *facade* (crate present but not doing the work), or
   *under-used* (the crate could do more than the implementation uses).
2. **Compile-fail audit**: every case you disagree with (wrong class, first
   error not the guarantee, missing probe that the API could attempt, a
   `.stderr` that doesn't match), with the fix.
3. **NOTES.md claims**: spot-check at least five concrete claims against the
   code and crate source; list the ones that are wrong or overstated.
4. **Test integrity**: `tests/spec.rs` and `tests/compile_fail.rs` compared
   with another slot's (they started identical apart from the crate name);
   `NOT_EXPRESSED` entries and `#[ignore]`s justified or not.
5. **Benchmark distortions**: per-event allocation, cloning, `Err`
   construction, queues, `dyn` dispatch that the crate does not require.
6. **Required fixes**: a numbered list, each concrete enough for the
   implementer to act on without asking. Mark each *must* (changes a
   result) or *should* (quality).
7. **Overall**: one paragraph: does this implementation represent the crate
   fairly enough to rank it?

Your final message to me: per crate, the per-target grades, the count of
must-fixes, and the two most important findings, under 250 words in total.
