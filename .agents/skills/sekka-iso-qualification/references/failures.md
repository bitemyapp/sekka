# Q7 — diagnose, fix, rebuild, restart

1. Mark the candidate failed with precise steps, expected/actual outcome,
   screenshot, journal/serial log and source/hash. Preserve the original disk
   or snapshot before diagnostic changes. Verify a failure against current
   `main` in the affected repository before attributing it there.
2. Isolate ISO packaging/boot/Genkan/Tatami defects in Sekka, Aizome shell/app/
   greeter defects in Aizome, and toolkit defects in Sidestep. Check the actual
   dependency branch and revision; do not assume the three repositories share
   a lock or release schedule.
3. Create focused topic branches and PRs with the reproduction, root cause,
   change, meaningful regression check and limits of validation. User
   authorization to fix bugs includes creating the requested PRs; follow the
   active session's merge authorization and repository review requirements.
4. A synthetic VM or patched installed guest can validate a hypothesis. Record
   it as diagnostic/component evidence. Do not patch the running guest and
   certify its original ISO. Never suppress mount errors, disable the greeter,
   skip a broken app or relabel a failed feature as optional to get green.
   Follow [component comparison provenance](component-tests.md) for original/fixed
   builds, harness corrections and evidence carried across commit/rebase boundaries.
5. Once the discovered fixes are in the candidate source and dependencies,
   build a clean, newly identified ISO. Pin the dependency fixes in Sekka and
   preserve source provenance. Rerun Q0–Q6 on that exact hash, including already
   passed cases; each candidate ledger starts with `not-run`.
6. Continue until every required case has evidence. For long processes inspect
   their live PID/session/job and output; resume the same handle after an
   observation timeout. A missing/terminal handle justifies restart after
   investigation. Store exact resume commands and outstanding IDs between turns.
7. If hardware, credentials or external failure prevents a required case,
   record a blocker and continue independent work. Do not certify the candidate
   or claim the goal complete while required evidence is missing.
