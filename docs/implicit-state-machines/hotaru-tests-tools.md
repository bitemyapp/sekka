# HTT: Hotaru engine tests and development tools

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `apps/hotaru/crates/*/tests, apps/hotaru/tools`. Audited at `8e0acbe5`; line numbers refer to that commit.

102 files, 0 pending. 28 implicit machines: 2 High, 7 Medium, 19 Low.

## Machines

<a id="htt-01"></a>

### HTT-01 — (test harness) Clipboard transfer pump/poll drivers
- **Where:** `apps/hotaru/crates/hotaru-io/tests/clipboard.rs:22` (`receive_until_closed`); also `clipboard.rs:54` (paired writer/reader pump loop), `clipboard.rs:207` (last-writer EOF fixture)
- **Encoding:** `TransferState` returned by `pump()`, `reader.deadline()` as the poll timeout, local `observed_backpressure` flag and a 10 s `deadline`, `writer.bytes_written()` progress comparison
- **States:** `Pending` (poll for POLLIN) → `Complete` | `Cancelled` | `TimedOut` | `LimitExceeded` | `Failed`; paired loop: `Pumping` → `BackpressureObserved` → `ReaderComplete`
- **Transitions:** `pump(4096)` non-Pending returns (`clipboard.rs:24-27`); poll wait bounded by reader deadline (`clipboard.rs:33-38`); paired loop runs while reader Pending (`clipboard.rs:56`), stall assert (`:57`), backpressure detected when writer stays Pending without progress (`:60-62`), short reader pump (`:65`); duplicate write end dropped to release EOF (`:229-230`)
- **Hazards:** `receive_until_closed` has no deadline of its own and relies on `pump` enforcing `TimedOut`; `reader.fd().unwrap()` (`:29`) assumes `Pending ⇒ fd is Some`; the paired loop never inspects the writer's terminal state until after the loop
- **Reify:** Low — local test loops over an already explicit `TransferState`
- **Related:** hotaru-io `apps/hotaru/crates/hotaru-io/src/clipboard.rs` (`TransferState`, `ClipboardReader/Writer::pump`)

<a id="htt-02"></a>

### HTT-02 — (test harness) SIGPIPE child re-exec and signal-mask fixtures
- **Where:** `apps/hotaru/crates/hotaru-io/tests/clipboard.rs:117` (`HOTARU_SIGPIPE_TEST_CHILD`); also `clipboard.rs:148` (mask/pending sequence)
- **Encoding:** env var selects role (parent vs child) for the same test function; thread signal mask + pending set (`set`, `previous`, `pending`) across an unsafe block
- **States:** `Parent` → spawn self (`--exact`, one thread) → `Child(SIG_DFL)` → exit status; mask: `Unblocked` → `Blocked+Pending` → `Pumped` → `Checked` → `Drained` → `Restored`
- **Transitions:** role check (`clipboard.rs:118`); re-exec (`:119-127`); success assert (`:128-133`); child sets `SIG_DFL` (`:138-140`) and runs inner test (`:141`); block (`:153-156`), `pthread_kill` (`:157`), pump (`:162`), `sigpending` (`:164-165`), drain `sigtimedwait` (`:172`), restore (`:173`), asserts (`:174-176`)
- **Hazards:** if the test is renamed without updating the `--exact` string, the child runs zero tests and exits 0, so the parent passes without testing anything; a panic in `pipe()` (`:10`, `:18`) between block and restore leaves the test thread's mask blocked (thread then ends); drain result unchecked
- **Reify:** Low — two small, local sequences

<a id="htt-03"></a>

### HTT-03 — (test harness) PTY child lifecycle waits
- **Where:** `apps/hotaru/crates/hotaru-io/tests/pty.rs:12` (`collect`); also `pty.rs:52`, `:98`, `:132`, `:144`, `:188`, `:200`
- **Encoding:** per-test `deadline: Instant` reused across phases, `Pty::read` result (`Ok(0)` = EOF via EIO mapping), `try_wait()`, `/proc/{pid}` existence
- **States:** `Spawned` → `Ready` (first output byte) → `Interacting` → `EOF` → `Exited` → `Reaped`
- **Transitions:** read loop to EOF with poll on WouldBlock (`pty.rs:16-35`); exit wait via `try_wait` (`:52-60`, `:200-203`); readiness waits on first output (`:98-106`, `:132-140`, `:188-196`); drop then poll `/proc` until reaped (`:142-147`)
- **Hazards:** `collect` depends on `Pty::read` turning EIO into `Ok(0)`; later phases reuse an earlier `deadline` so they get only the remaining budget; `/proc/{pid}` polling can be fooled by PID reuse (negligible)
- **Reify:** Low — test-local wait loops
- **Related:** hotaru-io `apps/hotaru/crates/hotaru-io/src/pty.rs:213-226` (EIO→EOF), `Drop` for `Pty`

<a id="htt-04"></a>

### HTT-04 — (test harness) Shell-startup PTY driver (parent/child re-exec)
- **Where:** `apps/hotaru/crates/hotaru-io/tests/shell.rs:190` (child loop); parent `shell.rs:36`
- **Encoding:** string `mode` (`integrated|hostile|array|hook-reset|disabled|explicit`) passed via `HOTARU_TEST_CHILD`; child loop locals `output`, `responses`, `response_at`, `sent`, `deadline`, a `hotaru_core::Terminal` answering queries
- **States:** `Parent(mode)` → `Child` → `AwaitingPrompt` → `SendingInput` (`sent < input.len()`) → `InputSent` → `ChildExited` → `Drained`; terminal replies preempt input
- **Transitions:** per-shell mode skips (`shell.rs:45-50`); re-exec with disposable env (`:88-107`); child role gate (`:170-172`); mode dispatch (`:175-180`); read feeds terminal and queues replies (`:196-205`); reply writes advance `response_at` (`:206-211`); prompt `HOTARU> ` seen → input writes advance `sent` (`:212-221`); exit detected → drain → break (`:222-231`); EOF break (`:197`)
- **Hazards:** prompt detection rescans the whole output each 2 ms tick and stays true once seen, so the whole script is pushed after the first prompt; input is withheld whenever any reply bytes are pending; the exit drain stops at the first WouldBlock and does not feed the terminal; the EOF path skips `try_wait`
- **Reify:** Low — fixture-only, single loop
- **Related:** hotaru-io `apps/hotaru/crates/hotaru-io/src/pty.rs` (`PtyCommand::shell`, `integrate_shell`); `apps/hotaru/resources/shell-integration/`

<a id="htt-05"></a>

### HTT-05 — (dev tool) Shared workstation measurement lock
- **Where:** `apps/hotaru/tools/benchmark.py:48` (`--lock` default `~/.cache/hotaru-workstation.lock`), `benchmark.py:74-75`; also `build-candidate.py:53-54`, `build-reference-adapter.py:29-30`, `frame-benchmark.py:685-687`, `frame-harness-check.py:141-142`, `frame-matrix.py:325-326` (+ private `child.lock` `:317`, `:341`), `gpu-buffer-check.py:86-87`, `gui-probe.py:315-317`, `history-budget-benchmark.py:75-76`, `:80-81`, `:104-105`, `apps/hotaru/tools/history-hyperlink-capacity.py:38-39`, `horizontal-margins.py:85-86`, `apps/hotaru/tools/hyperlink-benchmark.py:83-84`, `kitty-clipboard-oracle.py:491-492`, `malformed-utf8-profile.py:33-34`, `apps/hotaru/tools/measure.py:57-58`, `apps/hotaru/tools/reference-gui.py:58-59`, `apps/hotaru/tools/reference.py:56-57`, `unicode-mode-benchmark.py:35-36`, `apps/hotaru/tools/unicode-profile.py:49-50`; documented-but-not-taken in `content-capture.py:5`, `search-benchmark.py:4`, `search-compare.py:4`
- **Encoding:** advisory `fcntl.flock(LOCK_EX)` on an open file description, held for the `with` block; frame-matrix substitutes a per-batch `child.lock` for its harness children
- **States:** `Free` → `Held(tool)` → `Free`; history-budget-benchmark releases between corpus generation, timing and held-capture sections
- **Transitions:** acquire at each `with lock.open('a')` + `flock`; release at block exit; frame-matrix passes `--lock output/child.lock` so `frame-benchmark.py` never re-acquires the shared lock (`frame-matrix.py:341`)
- **Hazards:** blocking flock with no timeout; nested acquisition by a child using the default path would hang forever (flock is per open description); `horizontal-margins.py:85` and `kitty-clipboard-oracle.py:491` hard-code `/home/callen/...` and `apps/hotaru/tools/history-hyperlink-capacity.py:38` ignores `--lock`, so they fall out of exclusion under another HOME; `reference.py` holds the lock across `git clone`/network downloads; several timing tools take no lock at all
- **Reify:** Medium — one cross-tool mutex re-implemented in ~20 files with inconsistent paths and coverage
- **Suspected bug:** `paired-feed-benchmark.py` times two builds (`apps/hotaru/tools/paired-feed-benchmark.py:66-71`) without taking the lock, so a `build-candidate.py` compile can run concurrently and skew the ratio; load averages are recorded but not used as a gate (confidence: medium)

<a id="htt-06"></a>

### HTT-06 — (dev tool) Validated alternating benchmark round loops
- **Where:** `apps/hotaru/tools/benchmark.py:125` (comparability gate); also `apps/hotaru/tools/hyperlink-benchmark.py:117-123`, `malformed-utf8-profile.py:51-55`, `unicode-mode-benchmark.py:53-57`, `apps/hotaru/tools/unicode-profile.py:70-74`, `paired-feed-benchmark.py:58-71`, `apps/hotaru/tools/search-compare.py:53-59`, `search-benchmark.py:94`, `history-budget-benchmark.py:82-94`, `content-capture.py:58-71`, `apps/hotaru/tools/measure.py:61-91`
- **Encoding:** pre-timing equality result (`delta`, `unsupported`, asserts), warmup call or negative round index (`apps/hotaru/tools/search-compare.py:53`), round parity choosing order, `order` list, per-engine `samples`, post-run hash re-check
- **States:** `Validate` → `NotComparable` (report, exit 1) | `Warmup` → `Round(i, forward|reversed)` → `Aggregate` → `ProvenanceRecheck` → `Report`
- **Transitions:** gate (`benchmark.py:125`); warmup (`:158-159`); ABBA rounds (`:160-166`); medians (`:167-172`); hash re-check (`:173-178`); failure exit (`:184-185`)
- **Hazards:** `malformed-utf8-profile.py:45` and `apps/hotaru/tools/unicode-profile.py:63` key samples by `revision` text, so two builds with the same label merge silently; `search-benchmark.py:94` alternates by needle index only, so each case always runs in the same order; `content-capture.py:61` uses a fixed order; `history_benchmark.py` (no alternation) is outside this pattern
- **Reify:** Low — local loops; a shared helper would remove drift

<a id="htt-07"></a>

### HTT-07 — (dev tool) Build provenance metadata gate (`build_confirmed`, hashes)
- **Where:** `apps/hotaru/tools/build-candidate.py:83` (`build_confirmed: True`, `binary_sha256` `:87`, `sources` `:86-87`); writers also `apps/hotaru/tools/reference.py:115`, `build-reference-adapter.py:33-39`; consumers `benchmark.py:61-69`, `differential.py:419-423`, `input-oracle.py:37-39`, `apps/hotaru/tools/hyperlink-benchmark.py:69-79`, `unicode-mode-benchmark.py:26-29`, `malformed-utf8-profile.py:28-32`, `apps/hotaru/tools/unicode-profile.py:45-47`, `history-budget-benchmark.py:53-54`, `apps/hotaru/tools/history-hyperlink-capacity.py:47`, `frame-matrix.py:337-338`
- **Encoding:** JSON metadata files (candidate metadata, `upstream/baseline-build.json`, reference `hotaru-baseline.json`, history `build.json`) with `build_confirmed`, binary/source SHA-256s; consumers re-hash on disk
- **States:** `Unbuilt` → `Recorded(unconfirmed)` → `Confirmed(hash)` → `Stale` (bytes changed; noticed only where a consumer re-hashes)
- **Transitions:** candidate: lock, source hash, `cargo clean`, build, source re-check, write (`build-candidate.py:53-94`); reference: `build` vs `metadata` action (`apps/hotaru/tools/reference.py:93-123`); adapter rewrites baseline binaries (`build-reference-adapter.py:31-39`); consumer checks before and after timing (`benchmark.py:63-69`, `apps/hotaru/tools/benchmark.py:173-178`; `apps/hotaru/tools/hyperlink-benchmark.py:125-126`; `apps/hotaru/tools/unicode-profile.py:77-79`)
- **Hazards:** consumers are inconsistent (some check `build_confirmed` plus hashes, `unicode-mode-benchmark.py` checks hashes only, oracles check the library hash only); `upstream/baseline-build.json` is a hand copy of the reference's `hotaru-baseline.json`; `build-reference-adapter.py:32` uses a bare `assert`, which `-O` strips
- **Reify:** Medium — persisted gate with many independent consumers
- **Suspected bug:** `benchmark.py` validates candidate metadata against the binary (`apps/hotaru/tools/benchmark.py:63`) before taking the lock (`apps/hotaru/tools/benchmark.py:74-75`), then re-hashes inside the lock (`apps/hotaru/tools/benchmark.py:76`) and later compares only against that in-lock hash (`apps/hotaru/tools/benchmark.py:173-178`). If `build-candidate.py` holds the lock while benchmark.py waits and rebuilds `hotaru-replay`, the new binary is measured but the report records the stale metadata's source hash and toolchain (`apps/hotaru/tools/benchmark.py:104-108`), and the end check still passes (confidence: medium)
- **Related:** HTT-26 (reference build stages)

<a id="htt-08"></a>

### HTT-08 — (dev tool) libghostty-vt ctypes terminal session lifecycle
- **Where:** `apps/hotaru/tools/differential.py:179` (`Oracle.snapshot`), `differential.py:162` (`formatted`); also `cursor-oracle.py:65-90`, `hyperlink-oracle.py:184-258`, `apps/hotaru/tools/hyperlink-benchmark.py:23-60`, `input-oracle.py:160-186`, `reflow.py:45-106`, `apps/hotaru/tools/search-oracle.py:144-180`, `search-benchmark.py:28-58`, `apps/hotaru/tools/unicode-mode-oracle.py:77-109`, `apps/hotaru/tools/unicode-mode-oracle.py:128`, `kitty-clipboard-oracle.py:384-474` (HTT-24)
- **Encoding:** `c_void_p` handles (terminal, formatter, search, render state), ctypes `CFUNCTYPE` WRITE_PTY callback held only by a Python local, `replies` bytearray, two-call buffer sizing (`-3` = out of space)
- **States:** `Allocated` → `Configured` → `CallbackInstalled` → `Fed` → `Hooked` (after_feed/capture) → `Queried` → `Freed`; formatter/search: `New` → `SizeProbe` → `Filled` → `Freed`
- **Transitions:** new (`differential.py:181`); options (`:184-190`); callback (`:192-194`); feed (`:195-197`); after_feed (`:198-199`); processing-error gate (`:201-202`); replies captured (`:204`); capture hook (`:251-252`); free in `finally` (`:254-255`); size probe/fill (`:170-174`); search handle freed only if allocated (`apps/hotaru/tools/search-oracle.py:178-179`); replies cleared per step (`reflow.py:73`)
- **Hazards:** callback objects must outlive the terminal; this holds today only because free happens in the same frame (`cursor-oracle.py:69-71` rebinds `cb` per case); `VT_PROCESSING_ERROR` is checked only in `differential.snapshot` and the kitty run, not in cursor/hyperlink/search/reflow/input oracles, which record fixtures from an unchecked reference terminal; replies produced by the `capture` hook after `apps/hotaru/tools/differential.py:204` are not recorded
- **Reify:** Low — could be one context manager; dev-only
- **Related:** external pinned libghostty-vt C API; Rust consumers `apps/hotaru/crates/hotaru-core/tests/*.rs` fixtures

<a id="htt-09"></a>

### HTT-09 — (dev tool) Frame benchmark PTY fixture protocol
- **Where:** `apps/hotaru/tools/frame-benchmark.py:105` (`fixture`); `startup` `:128`, `armed` `:118`
- **Encoding:** `startup: bool`, `armed` list of pre-rendered keyboard trials, `select` over PTY stdin, control socket and SIGWINCH wakeup pipe (`:109-111`, `:132`), JSON-line ops/events
- **States:** `Connecting` → `Startup` (repaint seq 0 on resize) → `Driven` {`Disarmed` | `Armed(n)`} ; blocking `Streaming` / `Throughput` ; → `Quit` | `EOF`
- **Transitions:** connect + `ready` (`:112-117`); initial paint if ≥76x4 (`:129-130`); SIGWINCH repaint while startup (`:133-137`); `paint` clears startup (`:144-146`); `arm` fills trials (`:147-151`); key byte pops a trial (`:189-193`); `stream` paced paints (`:152-164`); `throughput` (`:165-179`); `quit` (`:180-181`); PTY EOF (`:187-188`)
- **Hazards:** during `stream`/`throughput` the loop services neither PTY input nor SIGWINCH; any input byte other than `a`, or `a` with nothing armed, raises and kills the fixture (`:190-191`); only `paint` clears `startup`, so the controller must send `paint` before other ops (it does, `:536`)
- **Reify:** Medium — cross-process protocol whose ordering rules live in the controller
- **Related:** HTT-10 (controller), HTT-13 (harness check); terminals under test (Hotaru app, Ghostty)

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

<a id="htt-11"></a>

### HTT-11 — (dev tool) JSON event reader thread and `until` waits
- **Where:** `apps/hotaru/tools/frame-benchmark.py:206` (`JsonReader`: `events`, `queue`, `failure`, `file`, `thread`); users `frame-benchmark.py:459`, `:489`, `frame-harness-check.py:60`, `:110`
- **Encoding:** daemon reader thread, unbounded queue, single `None` sentinel on failure, `failure` exception
- **States:** `Reading` → `Failed(failure)`; per `until`: `Waiting` → `Matched` | `ReaderEnded` | `TimedOut`
- **Transitions:** append + enqueue (`apps/hotaru/tools/frame-benchmark.py:218-223`); failure sentinel (`apps/hotaru/tools/frame-benchmark.py:224-226`); `until` consumes and discards non-matching events (`apps/hotaru/tools/frame-benchmark.py:230-238`); timeouts (`apps/hotaru/tools/frame-benchmark.py:233-234`, `apps/hotaru/tools/frame-benchmark.py:239`)
- **Hazards:** `until` discards non-matching queued events (they remain in `.events`), so a waiter for B loses an earlier A for later waits; only one sentinel, so a second waiter after failure waits the full timeout; EOF is reported as "missing/oversized" (`apps/hotaru/tools/frame-benchmark.py:52-56`); `file` is closed after a 2 s join while the thread may still write (`apps/hotaru/tools/frame-benchmark.py:637-638`)
- **Reify:** Low

<a id="htt-12"></a>

### HTT-12 — (dev tool) Frame qualification → calibration → matrix → summary pipeline
- **Where:** `apps/hotaru/tools/frame-calibration.py:22` (problems gate), `:50` (`qualified_for_future_matched_measurements`); also `frame-benchmark.py:403`, `:541`, `:625`; `frame-matrix.py:292-301`, `:335-338`, `:366-379`; `frame-summary.py:33-35`, `:77-79`
- **Encoding:** persisted `result.json` (`kind`, `purpose`, `status`, `physical_geometry_qualified`, `renderer`, `gpu.ghostty.gpu_presentation`, `fonts`, `running_executable_sha256`), calibration `summary.json` `status`, matrix `result.json` `status` + per-trial `accepted`
- **States:** `QualificationRun(passed|failed)` ×2 → `Calibration(qualified|not_qualified)` → `MatrixBatch(running→passed|stopped_*|failed)` per phase → `Summary`
- **Transitions:** calibration checks (`frame-calibration.py:22-37`), output creation (`:38`), summary write (`:49-53`); matrix calibration gate (`frame-matrix.py:292-294`) and copy (`:295-301`); per-trial identity/geometry gates (`:366-379`); summary requires `passed` and all trials accepted (`frame-summary.py:33-35`)
- **Hazards:** the `status` expression at `frame-calibration.py:50` cannot be `not_qualified` (dead branch); the calibration binary is taken positionally from `commands.application[3]` (`:44`), which is right only when `taskset` prefixes the command; the summary's throughput gate records no compiler-overlap count
- **Reify:** Medium — string statuses passed between four tools through files

<a id="htt-13"></a>

### HTT-13 — (test harness) Frame fixture and observer qualification drivers
- **Where:** `apps/hotaru/tools/frame-harness-check.py:35` (`fixture_test`), `:94` (`protocol_test`), `:135` (`main`)
- **Encoding:** real PTY pair + drain thread + `captured` bytes; private Sway/observer processes; `JsonReader` waits
- **States:** fixture: `Ready` → `Startup(seq0)` → `ResizedRepaint(rows=28)` → `Painted(1)` → `Armed(3)` → `Keys` → `Streams` → `Quit` → `Verified`; observer: `Ready` → `Capturing` → `Stopped` → `Key` → `Restarted` → `Quit`
- **Transitions:** openpty/winsize (`:38-39`); accept/ready (`:57-62`); resize + SIGWINCH (`:63-65`); paint/arm/keys (`:66-73`); streams (`:74-78`); quit (`:79-80`); verification (`:82-86`); teardown (`:89-91`); observer start/stop/key/restart/quit (`:116-125`); decoder rebuild by command-list surgery (`:144-149`)
- **Hazards:** `os.close(master)` (`:91`) can run while the drain thread is still reading; the slave fd leaks if `Popen` raises; the stop check (`:118-120`) assumes 100 ms drains the one in-flight capture; the command surgery depends on `build_observer`'s argument layout
- **Reify:** Low

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

<a id="htt-15"></a>

### HTT-15 — (dev tool) Frame matrix trial process lifecycle and orphan cleanup
- **Where:** `apps/hotaru/tools/frame-matrix.py:216` (`run_trial`: `stop` Event `:217`, `samples`, `errors`); `cleanup_descendants` `:127`; `enable_subreaper` `:115`
- **Encoding:** harness `Popen` in its own session, monitor thread controlled by `stop`, `PR_SET_CHILD_SUBREAPER`, pidfd-based kills, `cleanup` list
- **States:** `Spawned` → `Monitoring` → `Exited` | `TimedOut` → `Interrupted(SIGINT)` → `Killed(SIGKILL pg)` → `OrphanCleanup` → `Done`
- **Transitions:** spawn (`:234`); monitor start (`:235`); wait 600 s (`:236`); SIGINT (`:241`) → 20 s (`:243`) → killpg (`:246`) → 5 s (`:247`); cleanup (`:248`: reap `:132-134`, scan `:135-136`, pidfd re-check + SIGKILL `:139-153`, 5 s bound `:129`, raise `:155`); stop monitor (`:249-253`); final sample (`:254-257`)
- **Hazards:** if `cleanup_descendants` raises (`:155`), `stop.set()` is skipped, the monitor keeps sampling, the final sample is lost, and the RuntimeError hides the original error; `process.wait(timeout=5)` (`:247`) can raise inside `finally`; a 600 s timeout propagates as a batch failure with no rejection receipt
- **Reify:** Medium
- **Related:** HTT-10 (child's SIGINT handling)

<a id="htt-16"></a>

### HTT-16 — (dev tool) Frame matrix preflight quiet-wait retry loop
- **Where:** `apps/hotaru/tools/frame-matrix.py:158` (`preflight`: `deadline` `:161`, `attempts` `:162`)
- **Encoding:** attempt counter, monotonic deadline, 1 s activity sample, 2 s backoff
- **States:** `Sampling` → `Quiet(passed)` | `Busy` → `Backoff` → `Sampling` | `DeadlineExpired(failed)`
- **Transitions:** sample window (`:165-168`); reasons (`:169-175`); return (`:176-178`); backoff (`:179`)
- **Hazards:** the deadline is checked only after a sample, so a call can overrun `--quiet-wait` by about 3 s; with `wait=0` a single second decides
- **Reify:** Low

<a id="htt-17"></a>

### HTT-17 — (dev tool) Frame observer screencopy capture cycle
- **Where:** `apps/hotaru/tools/frame-observer.c:39` (`running, quit, buffer_busy, formats_done, offered_shm`), `:40` (`next_ns, request_ns, capture_serial`), `:34` (`frame`), `:35` (`buffer`)
- **Encoding:** globals plus the `frame` pointer (non-NULL = capture in flight); SHM buffer allocated lazily once
- **States:** `Stopped` | `Idle(wait next_ns)` → `Requested(frame)` → `Offered(offered_shm)` → `Copying(buffer_busy)` → `Ready` → `Idle`; `Quit`; invalid but representable `buffer_busy && !frame` (only after `failed`)
- **Transitions:** `start` (`:227-229`), `stop` (`:230`), `quit` (`:232`); request (`:273-277`); `offered` + lazy buffer (`:150-176`, geometry change fatal `:163`); `buffer_done` → `try_copy` (`:203-207`, `:138-143`); `ready` clears frame/busy and schedules `next_ns` (`:180-192`); `failed` exits (`:193-196`); teardown (`:308-312`)
- **Hazards:** `stop` does not cancel an in-flight capture (controllers sleep to drain: `frame-benchmark.py:549`, `frame-harness-check.py:118`); `start` changes `nonce` mid-flight; `next_ns = request_ns + period` produces back-to-back captures when decoding is slow; any screencopy failure or geometry change ends the process
- **Reify:** Medium — five globals encode one lifecycle across callbacks
- **Related:** wlr-screencopy in Sway; controller HTT-10

<a id="htt-18"></a>

### HTT-18 — (dev tool) Frame observer marker geometry cache
- **Where:** `apps/hotaru/tools/frame-observer.c:41` (`gx = -1, gy, gw, gh, bottom_y`); also `apps/hotaru/tools/frame-observer-test.c:36`, `apps/hotaru/tools/frame-observer-test.c:50`
- **Encoding:** `gx < 0` means undiscovered; discovery writes all five ints and prints a `geometry` event
- **States:** `Undiscovered` → `Discovered(gx,gy,gw,gh,bottom_y)` → `reset` → `Undiscovered`; `gw/gh` are scribbled during failed scans while `gx == -1`
- **Transitions:** fast path (`apps/hotaru/tools/frame-observer.c:116`); scan (`apps/hotaru/tools/frame-observer.c:119-134`, assign `apps/hotaru/tools/frame-observer.c:128`); `reset` (`apps/hotaru/tools/frame-observer.c:231`); tests force rediscovery and invalid width (`apps/hotaru/tools/frame-observer-test.c:36`, `apps/hotaru/tools/frame-observer-test.c:50`)
- **Hazards:** the cache stays valid only until a resize; the controller must send `reset` (`frame-benchmark.py:535`); discovery picks the first matching bottom marker scanning downward (`:126-127`)
- **Reify:** Low
- **Suspected bug:** if the first capture after `reset` still shows pre-resize content (the grid grew, so the old bottom marker is now an interior row), discovery locks `bottom_y` to that row; sequence 1's marker on the new last row never decodes, and `observations.until(... sequence == 1)` (`frame-benchmark.py:538`) times out (confidence: low)

<a id="htt-19"></a>

### HTT-19 — (dev tool) GPU buffer qualification build/run variants
- **Where:** `apps/hotaru/tools/gpu-buffer-check.py:88` (variants `host`, `nix-loader`), `report["runs"]`
- **Encoding:** per-variant `entry` dict accumulating `build_returncode`, `returncode`, `events`, `loader_scope_qualified`; overall `report["status"]`
- **States:** per run `Building` → `BuildFailed` | `Running` → `TimedOut(killed)` | `Exited` → `Classified`; overall `passed|failed`
- **Transitions:** build (`:90-101`); spawn in new session (`:116-117`); 30 s wait (`:119`); timeout → killpg (`:121-130`); parse/classify (`:131-144`); status (`:145-148`); write (`:149`)
- **Hazards:** a second `communicate(timeout=5)` after SIGKILL (`:126`) can raise uncaught; every stdout line is parsed as JSON (`:133`), so stray driver output aborts without `result.json`
- **Reify:** Low

<a id="htt-20"></a>

### HTT-20 — (dev tool) EGL offscreen probe stage and current-context sequence
- **Where:** `apps/hotaru/tools/gpu-buffer-probe.c:86` (`producer`, `consumer`); also `apps/hotaru/tools/renderer-discovery.py:44-131`
- **Encoding:** implicit EGL current context, resource handles created per stage, `failure(stage)` exits
- **States:** current `NONE` → `producer` → `consumer` → `producer` → `NONE`; stages init → config → contexts → draw/readback → export → fence → import → readback → teardown → `passed`
- **Transitions:** contexts + producer current (`apps/hotaru/tools/gpu-buffer-probe.c:86-89`); export (`apps/hotaru/tools/gpu-buffer-probe.c:146-160`); fence (`apps/hotaru/tools/gpu-buffer-probe.c:162-165`); consumer current (`apps/hotaru/tools/gpu-buffer-probe.c:166`); import + close fd (`apps/hotaru/tools/gpu-buffer-probe.c:172-174`); teardown order (`apps/hotaru/tools/gpu-buffer-probe.c:192-199`); failure exit (`apps/hotaru/tools/gpu-buffer-probe.c:40-46`); renderer-discovery init (`apps/hotaru/tools/renderer-discovery.py:89-93`), try (`apps/hotaru/tools/renderer-discovery.py:94-118`), cleanup (`apps/hotaru/tools/renderer-discovery.py:121-127`)
- **Hazards:** GL calls act on whichever context is current, so deletes must happen before switching (`apps/hotaru/tools/gpu-buffer-probe.c:192-196`); failure paths rely on process exit to release the exported fd
- **Reify:** Low

<a id="htt-21"></a>

### HTT-21 — (dev tool) GUI probe private session and DSR fixture
- **Where:** `apps/hotaru/tools/gui-probe.py:114` (`probe`); fixture `gui-probe.py:46`
- **Encoding:** `processes`/`logs` lists; calibration `width`/`height`; fixture request loop with `response` bytes and a 30 s DSR deadline
- **States:** controller `BringUp` → `Calibrating` → `IdleSampling` → `Workload(i)` → `Done`; fixture `Ready` → `Idle` ↔ `Writing` → `AwaitingDSR` → `Replied`
- **Transitions:** D-Bus/Sway (`:158-173`); app + accept (`:200-206`); window mapped (`:207-211`); calibration ≤120 (`:219-231`); idle sample (`:232-237`); workloads (`:261-272`); write result (`:273`); teardown (`:279-292`); fixture drain/write/DSR wait (`:61-80`)
- **Hazards:** `result.json` is written only on success; workloads containing queries fail the exact DSR check (`:77-78`); the teardown has the same `TimeoutExpired` exposure as HTT-10
- **Reify:** Low
- **Related:** HTT-10 (same private-session pattern)

<a id="htt-22"></a>

### HTT-22 — (dev tool) Input oracle shared encoder/event handles across cases
- **Where:** `apps/hotaru/tools/input-oracle.py:72` (`handles`: `ke, kk, me, mm`)
- **Encoding:** one key event, key encoder, mouse event and mouse encoder reused for every generated case; `seen` set dedupes rows
- **States:** per handle `Created` → `Configured(last case)` → … → `Freed`; negotiation inherits mouse options from the final mouse case
- **Transitions:** per key case full reset (`:91-103`); mouse reset + options (`:130-132`); negotiation `setopt_from_terminal` without mouse reset (`:166`, `:177`); frees in reverse (`:187-189`)
- **Hazards:** negotiation expectations depend on leftover `ANY_BUTTON_PRESSED=True` (last set at `:131` with button 9) and last-cell state from earlier encodes; the Rust side mirrors this with `any_pressed=true` (`hotaru-core/tests/input.rs:279`); reordering loops would silently change fixtures
- **Reify:** Low
- **Related:** hotaru-core `apps/hotaru/crates/hotaru-core/tests/input.rs:227-285`

<a id="htt-23"></a>

### HTT-23 — (dev tool) Upstream port-ledger per-file status
- **Where:** `apps/hotaru/tools/inventory.py:44` (`entry["status"]`), output `upstream/modules.json`
- **Encoding:** per-file `status` (`pending|excluded`, plus hand-set values such as `partial`), `evidence`, `rust_paths`, keyed by upstream `blob` and `scope`
- **States:** `pending` → hand-edited (`partial`, …) ; `excluded`; any → `pending` when blob or scope changes
- **Transitions:** load previous (`apps/hotaru/tools/inventory.py:34-36`); default status (`apps/hotaru/tools/inventory.py:45`); retain when blob and scope match (`apps/hotaru/tools/inventory.py:47-50`); write (`apps/hotaru/tools/inventory.py:57`)
- **Hazards:** hand-set statuses are unvalidated strings; evidence for removed upstream paths is dropped silently; `--revision` is not checked against the tree listing
- **Reify:** Low

<a id="htt-24"></a>

### HTT-24 — (dev tool) Kitty clipboard oracle scripted embedder
- **Where:** `apps/hotaru/tools/kitty-clipboard-oracle.py:350` (`Ghostty`: `entropy`, `entropy_at`, `entropy_requests` `:367`); `run` `:384`
- **Encoding:** process-global RANDOM_SECURE callback reading an entropy cursor; per run `clipboards` per location, `reads`/`writes` decision queues, `step` accumulator, `keep` buffers
- **States:** `Allocated` → `CallbacksInstalled` → `Step(i){Feed|Paste}` → `Checked` → `Freed`; decision-queue and entropy cursors advance; clipboards change on successful writes
- **Transitions:** global RNG install (`:369-379`); new (`:387`); callbacks (`:442-445`); write limit (`:446-448`); entropy reset (`:449`); step reset/accumulate (`:452-467`); decisions pop (`:412`, `:430`); clipboard update (`:431-432`); processing-error gate (`:468-471`); free (`:473-474`)
- **Hazards:** the entropy reset (`:449`) comes after `terminal_new` and callback install, so randomness drawn during creation would read the previous case's leftover entropy and go unrecorded; `step` is `None` before the first step; exhausted decision queues default to success
- **Reify:** Low
- **Related:** hotaru-replay `apps/hotaru/crates/hotaru-replay/src/bin/kitty-clipboard-replay.rs` (same embedder model), `hotaru-replay/tests/kitty_clipboard.rs`

<a id="htt-25"></a>

### HTT-25 — (dev tool) Process-timing run watchdog
- **Where:** `apps/hotaru/tools/measure.py:67` (`expired` Event), `apps/hotaru/tools/measure.py:75` (`threading.Timer`)
- **Encoding:** per run `Popen` in a new session, timer thread, `expired` flag, warmup decided by `index < warmup`
- **States:** `Warmup|Measured` × (`Running` → `Exited` | `Expired(killed)`)
- **Transitions:** spawn (`apps/hotaru/tools/measure.py:66`); timer → `poll()` → set + killpg (`apps/hotaru/tools/measure.py:68-77`); wait/cancel/join (`apps/hotaru/tools/measure.py:78-82`); expired → SystemExit (`apps/hotaru/tools/measure.py:83-84`); sample kept if measured (`apps/hotaru/tools/measure.py:88-91`); hash re-check (`apps/hotaru/tools/measure.py:92-93`)
- **Hazards:** `poll()` from the timer cannot take Popen's waitpid lock while the main thread is in `wait()`, so it always reports "running"; a process exiting right at the deadline is still counted as expired and its group signalled
- **Reify:** Low

<a id="htt-26"></a>

### HTT-26 — (dev tool) Pinned reference oracle prepare/build/metadata stages
- **Where:** `apps/hotaru/tools/reference.py:39` (`action`), `apps/hotaru/tools/reference.py:115` (`build_confirmed`); also `apps/hotaru/tools/reference-gui.py:58-124`
- **Encoding:** reference checkout HEAD and cleanliness, cached Zig archive/`.partial`/extracted dir, `hotaru-baseline.json` (`build_confirmed`, binary hashes), `hotaru-gui-baseline.json`
- **States:** `Absent` → `Cloned` → `PinnedClean+Toolchain` → `Built(confirmed)` ; `metadata` rewrite → `Recorded(unconfirmed)`
- **Transitions:** clone (`apps/hotaru/tools/reference.py:59-60`); refuse edits (`apps/hotaru/tools/reference.py:61-62`); fetch/detach pin (`apps/hotaru/tools/reference.py:63-64`); fork fetch (`apps/hotaru/tools/reference.py:65-66`); download → verify → replace (`apps/hotaru/tools/reference.py:67-77`); extract (`apps/hotaru/tools/reference.py:78-79`); version gate (`apps/hotaru/tools/reference.py:80-81`); pin/clean/version gates (`apps/hotaru/tools/reference.py:83-89`); build (`apps/hotaru/tools/reference.py:93-100`); hash binaries (`apps/hotaru/tools/reference.py:101-108`); write (`apps/hotaru/tools/reference.py:109-123`); GUI build gates (`apps/hotaru/tools/reference-gui.py:60-89`) and write (`apps/hotaru/tools/reference-gui.py:123-124`)
- **Hazards:** running `metadata` after `build` overwrites the file with `build_confirmed: false`; consumers read a separately committed copy (`upstream/baseline-build.json`); an interrupted extraction can leave a `zig` that passes the existence check; untracked `zig-out` artifacts from older revisions get hashed
- **Reify:** Medium — multi-invocation persisted workflow behind one flag
- **Related:** HTT-07

<a id="htt-27"></a>

### HTT-27 — (dev tool) Two-phase differential report → compact fixture generators
- **Where:** `apps/hotaru/tools/stream-parser-oracle.py:172`; also `vt-controls-oracle.py:112-141`
- **Encoding:** monkey-patched `differential.fixtures`, `report.json` written by phase 1, `--compact-fixtures` flag
- **States:** `ReportWritten(matched|reference-only)` → `FixturesWritten`; `ReportWritten(divergent|error)` → `Exit(1)` (no fixtures)
- **Transitions:** patch + run (`stream-parser-oracle.py:172-173`); `sys.exit(1)` on divergence (`differential.py:466-467`); re-read report (`:176-178`); reopen library from `report["library"]` (`:179`); regenerate and write (`:182-200`)
- **Hazards:** any candidate divergence blocks fixture regeneration (rerun without `--replay`); phase 2 does not re-verify the library against the `library_sha256` it copies into the fixture
- **Reify:** Low

<a id="htt-28"></a>

### HTT-28 — (dev tool) print_utf8 experiment variants (run accumulator, pending wrap)
- **Where:** `apps/hotaru/tools/unicode-experiment.py:63` (`STREAM_LOOP`: `run: Option<(start,left,id)>`, `pending`); also `:27-56` (`INNER_LOOP`: `run`, `count`, `stop_width`), `:93-116` (`DIRECT_LOOP`), `:59-61` (`WIDTH_CACHE`: `last_char`, `last_width`)
- **Encoding:** Rust templates spliced into a copied `screen.rs`/`row.rs`; open-run option, pending-wrap flag, last-width cache
- **States:** `NoRun` → `RunOpen` → `Flushed` (on zero-width, wrap or end); `pending` false → true at the right edge
- **Transitions:** run opened (`:75`); flushed on combining mark (`:69-71`) and at end (`:82-84`); pending set (`:79`); splice (`:143-171`)
- **Hazards:** `str.replace` splices (`:148-149`, `:169`) no-op silently if the source text differs, producing an unmodified "variant"; correctness relies on separate fixture checks (docstring `:5-6`)
- **Reify:** Low — generator for throwaway experiments
- **Related:** hotaru-core `apps/hotaru/crates/hotaru-core/src/screen.rs` (`print_utf8`)

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `apps/hotaru/crates/hotaru-core/examples/content_capture.rs` | 78 | none |  | capture timing driver; linear |
| `apps/hotaru/crates/hotaru-core/examples/search.rs` | 64 | none |  | search timing driver; linear |
| `apps/hotaru/crates/hotaru-core/examples/semantic_capture.rs` | 70 | none |  | F/R/C fixture op interpreter; stateless per op |
| `apps/hotaru/crates/hotaru-core/tests/clipboard_events.rs` | 159 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/configured_colors.rs` | 88 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/content.rs` | 139 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/core_review.rs` | 212 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/cursor.rs` | 127 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/decaln.rs` | 100 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/history.rs` | 102 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/history_audit.rs` | 149 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/history_budget.rs` | 338 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/horizontal_margins.rs` | 321 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/hyperlinks.rs` | 70 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/input.rs` | 369 | none |  | tests; mirrors HTT-22 residual any_pressed |
| `apps/hotaru/crates/hotaru-core/tests/kitty_clipboard.rs` | 535 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/reflow.rs` | 151 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/scan_boundary.rs` | 56 | none |  | tests; single mmap RAII guard |
| `apps/hotaru/crates/hotaru-core/tests/search.rs` | 185 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/search_positions.rs` | 283 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/semantic.rs` | 152 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/semantic_fastgate.rs` | 41 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/snapshot_threads.rs` | 72 | none |  | tests; scoped reader threads |
| `apps/hotaru/crates/hotaru-core/tests/terminal.rs` | 377 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/unicode_mode.rs` | 323 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/unicode_write.rs` | 122 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/utf8_scanning.rs` | 156 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/utf8_validation.rs` | 133 | none |  | tests; single mmap RAII guard |
| `apps/hotaru/crates/hotaru-core/tests/vt_controls.rs` | 120 | none |  | tests |
| `apps/hotaru/crates/hotaru-core/tests/working_directory.rs` | 49 | none |  | tests |
| `apps/hotaru/crates/hotaru-io/tests/clipboard.rs` | 302 | machines | [HTT-01](#htt-01), [HTT-02](#htt-02) | pump/poll drivers; SIGPIPE re-exec |
| `apps/hotaru/crates/hotaru-io/tests/pty.rs` | 206 | machines | [HTT-03](#htt-03) | PTY child wait loops |
| `apps/hotaru/crates/hotaru-io/tests/shell.rs` | 235 | machines | [HTT-04](#htt-04) | shell startup PTY driver |
| `apps/hotaru/crates/hotaru-replay/examples/history.rs` | 64 | none |  | RSS/read timing driver; linear |
| `apps/hotaru/crates/hotaru-replay/examples/reflow.rs` | 51 | none |  | JSON op runner; linear |
| `apps/hotaru/crates/hotaru-replay/tests/kitty_clipboard.rs` | 36 | none |  | tests; one subprocess per chunk |
| `apps/hotaru/crates/hotaru-replay/tests/upstream.rs` | 69 | none |  | tests; one subprocess per chunk |
| `apps/hotaru/crates/hotaru-sprites/tests/golden.rs` | 57 | none |  | tests; RLE fixture decode |
| `apps/hotaru/crates/hotaru-sprites/tests/raster.rs` | 148 | none |  | tests |
| `apps/hotaru/tools/benchmark.py` | 189 | machines | [HTT-05](#htt-05), [HTT-06](#htt-06), [HTT-07](#htt-07) | shared lock; ABBA rounds; provenance TOCTOU |
| `apps/hotaru/tools/build-candidate.py` | 100 | machines | [HTT-07](#htt-07), [HTT-05](#htt-05) | writes build_confirmed metadata |
| `apps/hotaru/tools/build-reference-adapter.py` | 43 | machines | [HTT-07](#htt-07), [HTT-05](#htt-05) | rewrites baseline binaries |
| `apps/hotaru/tools/capture-checkpoint.rs` | 38 | none |  | capture timing driver; linear |
| `apps/hotaru/tools/content-capture.py` | 76 | machines | [HTT-06](#htt-06), [HTT-05](#htt-05) | interleaved samples; lock only documented |
| `apps/hotaru/tools/corpus.py` | 36 | none |  | deterministic corpus generator |
| `apps/hotaru/tools/cursor-oracle.py` | 99 | machines | [HTT-08](#htt-08) | ctypes terminal/render-state sessions |
| `apps/hotaru/tools/differential.py` | 471 | machines | [HTT-08](#htt-08), [HTT-07](#htt-07) | Oracle session lifecycle; baseline gate |
| `apps/hotaru/tools/formatter-uri-audit.py` | 25 | none |  | independent HTML parse check |
| `apps/hotaru/tools/formatter-uri-audit.rs` | 31 | none |  | per-line stateless driver |
| `apps/hotaru/tools/frame-bench-shell.nix` | 30 | none |  | dev shell package list |
| `apps/hotaru/tools/frame-benchmark.py` | 691 | machines | [HTT-09](#htt-09), [HTT-10](#htt-10), [HTT-11](#htt-11), [HTT-12](#htt-12), [HTT-05](#htt-05) | fixture protocol; controller; reader thread |
| `apps/hotaru/tools/frame-calibration.py` | 59 | machines | [HTT-12](#htt-12) | qualification to calibration gate |
| `apps/hotaru/tools/frame-harness-check.py` | 168 | machines | [HTT-13](#htt-13), [HTT-11](#htt-11), [HTT-05](#htt-05) | fixture/observer qualification |
| `apps/hotaru/tools/frame-matrix.py` | 409 | machines | [HTT-14](#htt-14), [HTT-15](#htt-15), [HTT-16](#htt-16), [HTT-12](#htt-12), [HTT-05](#htt-05) | batch status; trial lifecycle; preflight |
| `apps/hotaru/tools/frame-observer-test.c` | 55 | machines | [HTT-18](#htt-18) | synthetic decoder tests; forces gx reset |
| `apps/hotaru/tools/frame-observer.c` | 314 | machines | [HTT-17](#htt-17), [HTT-18](#htt-18) | screencopy cycle; geometry cache |
| `apps/hotaru/tools/frame-summary.py` | 116 | machines | [HTT-12](#htt-12) | consumes matrix status and accepted gates |
| `apps/hotaru/tools/gpu-buffer-check.py` | 157 | machines | [HTT-19](#htt-19), [HTT-05](#htt-05) | build/run/timeout per variant |
| `apps/hotaru/tools/gpu-buffer-probe.c` | 202 | machines | [HTT-20](#htt-20) | EGL stages and current context |
| `apps/hotaru/tools/gui-probe.py` | 321 | machines | [HTT-21](#htt-21), [HTT-05](#htt-05) | private session and DSR fixture |
| `apps/hotaru/tools/history-budget-benchmark.py` | 119 | machines | [HTT-06](#htt-06), [HTT-05](#htt-05), [HTT-07](#htt-07) | ABBA groups; per-section lock |
| `apps/hotaru/tools/history-budget-candidate.rs` | 176 | none |  | linear audit; hold mode is config |
| `apps/hotaru/tools/history-budget-reference.c` | 44 | none |  | linear C audit adapter |
| `apps/hotaru/tools/history-hyperlink-capacity.c` | 53 | none |  | linear C probe |
| `apps/hotaru/tools/history-hyperlink-capacity.py` | 59 | machines | [HTT-05](#htt-05), [HTT-07](#htt-07) | lock ignores --lock; build sha checks |
| `apps/hotaru/tools/history-reference.c` | 28 | none |  | linear C adapter |
| `apps/hotaru/tools/history_benchmark.py` | 57 | none |  | sequential samples; post-hoc equality gate |
| `apps/hotaru/tools/horizontal-margins.py` | 118 | machines | [HTT-05](#htt-05), [HTT-08](#htt-08) | hardcoded lock path; oracle sessions |
| `apps/hotaru/tools/hyperlink-benchmark.py` | 132 | machines | [HTT-06](#htt-06), [HTT-07](#htt-07), [HTT-08](#htt-08), [HTT-05](#htt-05) | ABBA rounds; fingerprint recheck |
| `apps/hotaru/tools/hyperlink-oracle.py` | 265 | machines | [HTT-08](#htt-08) | ctypes sessions; HTML balancer is pure |
| `apps/hotaru/tools/hyperlink-probe.rs` | 20 | none |  | linear probe |
| `apps/hotaru/tools/input-oracle.py` | 197 | machines | [HTT-22](#htt-22), [HTT-08](#htt-08), [HTT-07](#htt-07) | shared encoder handles across cases |
| `apps/hotaru/tools/inventory.py` | 62 | machines | [HTT-23](#htt-23) | port ledger status retention |
| `apps/hotaru/tools/kitty-clipboard-oracle.py` | 535 | machines | [HTT-24](#htt-24), [HTT-08](#htt-08), [HTT-05](#htt-05) | scripted embedder; entropy cursor |
| `apps/hotaru/tools/malformed-utf8-profile.py` | 63 | machines | [HTT-06](#htt-06), [HTT-07](#htt-07), [HTT-05](#htt-05) | ABBA rounds keyed by revision |
| `apps/hotaru/tools/measure.py` | 113 | machines | [HTT-25](#htt-25), [HTT-06](#htt-06), [HTT-05](#htt-05) | timeout watchdog; warmup phase |
| `apps/hotaru/tools/nix-checkpoint.py` | 69 | none |  | one-shot package recorder |
| `apps/hotaru/tools/paired-feed-benchmark.py` | 87 | machines | [HTT-06](#htt-06), [HTT-05](#htt-05) | ABBA rounds; takes no lock |
| `apps/hotaru/tools/reference-gui.nix` | 8 | none |  | callPackage wrapper |
| `apps/hotaru/tools/reference-gui.py` | 129 | machines | [HTT-26](#htt-26), [HTT-05](#htt-05) | GUI baseline gates |
| `apps/hotaru/tools/reference-replay.c` | 173 | none |  | per-iteration bench adapter |
| `apps/hotaru/tools/reference.py` | 128 | machines | [HTT-26](#htt-26), [HTT-07](#htt-07), [HTT-05](#htt-05) | prepare/build/metadata stages |
| `apps/hotaru/tools/reflow.py` | 179 | machines | [HTT-08](#htt-08) | ctypes session; replies per step |
| `apps/hotaru/tools/renderer-discovery.py` | 158 | machines | [HTT-20](#htt-20) | per-device EGL probe cleanup |
| `apps/hotaru/tools/search-anchor-audit.rs` | 161 | none |  | tests |
| `apps/hotaru/tools/search-anchor-lookbehind-audit.rs` | 50 | none |  | tests |
| `apps/hotaru/tools/search-anchor-prefix-audit.rs` | 50 | none |  | tests |
| `apps/hotaru/tools/search-benchmark.py` | 114 | machines | [HTT-06](#htt-06), [HTT-08](#htt-08), [HTT-05](#htt-05) | needle-index order; lock only documented |
| `apps/hotaru/tools/search-compare.py` | 81 | machines | [HTT-06](#htt-06), [HTT-05](#htt-05) | negative-index warmup; lock only documented |
| `apps/hotaru/tools/search-oracle.py` | 188 | machines | [HTT-08](#htt-08) | search handle; two-call selections |
| `apps/hotaru/tools/stream-parser-oracle.py` | 200 | machines | [HTT-27](#htt-27), [HTT-08](#htt-08) | report then compact fixture |
| `apps/hotaru/tools/unicode-experiment.py` | 175 | machines | [HTT-28](#htt-28) | print_utf8 variant templates |
| `apps/hotaru/tools/unicode-kernel-oracle.py` | 98 | none |  | exhaustive comparison; pure |
| `apps/hotaru/tools/unicode-kernel-probe.rs` | 51 | none |  | per-request cluster probe over explicit BreakState |
| `apps/hotaru/tools/unicode-mode-benchmark.py` | 73 | machines | [HTT-06](#htt-06), [HTT-07](#htt-07), [HTT-05](#htt-05), [HTT-08](#htt-08) | ABBA rounds; hash-only gate |
| `apps/hotaru/tools/unicode-mode-oracle.py` | 140 | machines | [HTT-08](#htt-08) | snapshot with capture/after_feed hooks |
| `apps/hotaru/tools/unicode-profile.py` | 85 | machines | [HTT-06](#htt-06), [HTT-07](#htt-07), [HTT-05](#htt-05) | ABBA rounds keyed by revision |
| `apps/hotaru/tools/unicode-tables.py` | 76 | none |  | table generator; pure |
| `apps/hotaru/tools/unicode-write-regression.rs` | 67 | none |  | tests |
| `apps/hotaru/tools/vt-controls-oracle.py` | 141 | machines | [HTT-27](#htt-27), [HTT-08](#htt-08) | report then compact fixture |
| `apps/hotaru/tools/vt-controls-review.py` | 50 | none |  | fixture data generator for HTT-27 |
| `apps/hotaru/tools/wide-budget-audit.rs` | 23 | none |  | linear diagnostic |
