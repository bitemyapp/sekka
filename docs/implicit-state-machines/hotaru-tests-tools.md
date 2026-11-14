# HTT: Hotaru engine tests and development tools

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `apps/hotaru/crates/*/tests, apps/hotaru/tools`. Audited at `8e0acbe5`; line numbers refer to that commit.

102 files audited. 28 implicit machines: 2 High, 7 Medium, 19 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="htt-10"></a>

### HTT-10 — (dev tool) Frame benchmark controller run lifecycle
- **Where:** `apps/hotaru/tools/frame-benchmark.py:362` (`run`); `result["status"]` `:403`; `processes, logs, readers` `:402`
- **Encoding:** `result["status"]` (`running|passed|failed`) persisted to `result.json`; process/log/reader lists; sequence-number namespaces (0 startup, 1 initial, 100+ keys, 10000+1000·i streams, 50000+ throughput); `--purpose` gates
- **States:** `BringUp` (D-Bus → Sway → observer → app/fixture) → `Startup` → `GridCalibration` → `Qualified` → `ObserverControls` → `Keyboard` → `Streams` → `Throughput` → `Passed` | `Failed`; `Teardown` always
- **Transitions:** Uta gate (`:429-430`); D-Bus (`:435-437`); Sway sockets (`:442-446`); GPU renderer gate (`:447-453`); observer ready/start (`:458-465`); app launch/accept (`:485-486`); startup frame (`:490-505`); GPU presentation gate (`:508-516`); calibration ≤100 tries (`:524-534`); observer reset + paint 1 (`:535-538`); geometry gate (`:541-544`); controls (`:547-556`); keys (`:558-576`); streams (`:578-597`); throughput (`:599-619`); passed (`:625`); failed (`:626-628`); teardown SIGTERM→3 s→SIGKILL per group, joins, `result.json` (`:629-641`)
- **Hazards:** a second `TimeoutExpired` from `process.wait` after SIGKILL (`:636`) aborts `finally`, so remaining processes stay up, readers are not joined, no `result.json`, temp dir left; there is no liveness check on app or compositor, so a crash surfaces as a 30 s (600 s) `until()` timeout; calibration reads the size 30 ms after a resize and can oscillate on stale sizes
- **Reify:** High — many phases, persisted status, five processes
- **Suspected bug:** on frame-matrix's 600 s timeout the harness gets SIGINT (`frame-matrix.py:241`); `KeyboardInterrupt` bypasses `except Exception` (`apps/hotaru/tools/frame-benchmark.py:626`), so `finally` writes `result.json` still saying `"status": "running"` (`apps/hotaru/tools/frame-benchmark.py:403`, `apps/hotaru/tools/frame-benchmark.py:640`) for a terminated run (confidence: high)
- **Related:** HTT-09, HTT-11, HTT-17, HTT-15
- **Reproduction:** Confirmed (end-to-end): [summary](reproductions.md#htt-10)

<a id="htt-14"></a>

### HTT-14 — (dev tool) Frame matrix batch status and trial acceptance
- **Where:** `apps/hotaru/tools/frame-matrix.py:312` (`report["status"]`), round loop `:327`
- **Encoding:** `report["status"]` (`running|stopped_for_interference|preflight_passed_no_app_run|stopped_after_rejected_trial|passed|failed`) saved after every step (`save()` `:321-322`); per-trial receipts (`preflight`, `command`, `rejection_reasons`, `accepted`); round parity; `--start-round`
- **States:** `Running(round, kind)` → `PreflightFailed` (exit 75) | `PreflightOnly` (exit 0) | `TrialRejected` (exit 2) | `Passed` | `Failed`
- **Transitions:** preflight receipt (`:329-332`); preflight-only (`:333-334`); calibrated exe re-hash (`:337-338`); trial (`:363`); rejection reasons (`:366-389`); receipt (`:390-394`); rejected stop (`:395-396`); passed (`:397`); failed (`:398-399`); final save (`:400-402`)
- **Hazards:** resumption is only via `--start-round` into a fresh output dir (`:288`); interference detection knows only compiler process names (`COMPILERS` `:23`)
- **Reify:** High — multi-outcome persisted status with several gates
- **Suspected bug:** `--phase throughput` trials can be accepted even if an external compiler ran during the throughput flood. The measured interval ends at the last stream capture (`:380-382`), but frame-benchmark runs throughput after the streams (`frame-benchmark.py:599-619`), so `compiler_overlap` (`apps/hotaru/tools/frame-matrix.py:384`) never examines that window (confidence: high)
- **Related:** HTT-12, HTT-15, HTT-16
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#htt-14)
