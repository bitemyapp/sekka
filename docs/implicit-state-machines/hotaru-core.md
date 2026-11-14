# HCO: Hotaru terminal engine

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `apps/hotaru/crates`. Audited at `8e0acbe5`; line numbers refer to that commit.

41 files audited. 26 implicit machines: 7 High, 8 Medium, 11 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

<a id="hco-06"></a>

### HCO-06 — Kitty clipboard (OSC 5522) write transaction
- **Where:** `apps/hotaru/crates/hotaru-core/src/kitty_clipboard.rs:965` (`Protocol.write`); also `apps/hotaru/crates/hotaru-core/src/kitty_clipboard.rs:463` (`WriteState.current`, `.spool`, `.entries`, `.aliases`, `.decoder`), `apps/hotaru/crates/hotaru-core/src/kitty_clipboard.rs:372` (`base64::Streaming.carry`, `.carried`), `apps/hotaru/crates/hotaru-core/src/screen.rs:726` (`State.kitty_clipboard`)
- **Encoding:** `Option<WriteState>`; inside it the current MIME index `Option<usize>`, shared byte spool with per-type ranges, alias list, and a base64 carry of 0..3 bytes
- **States:** `Idle` → `Writing{current: None}` → `Writing{current: Some(i), carried: 0..3}` → `Committed` (reported as `Request::Write`) | `Failed(EFBIG|EINVAL)` → `Idle`; `MimeTableFull` (65th type silently dropped)
- **Transitions:** `type=write` starts/replaces silently (`kitty_clipboard.rs:1004`–`:1007`); `wdata` with type → `data` (`kitty_clipboard.rs:1015`; type switch finishes decoder `:511`–`:516`, reuse restarts range `:519`–`:522`, table full `:523`–`:525`, feed `:540`–`:542`); `walias` (`kitty_clipboard.rs:1022`–`:1027`); `wdata` without type commits (`kitty_clipboard.rs:1012`–`:1013`, `:1061`–`:1084`); errors abort (`kitty_clipboard.rs:1017`–`:1018`, `:996`, `:1087`–`:1091`); data with no write ignored (`kitty_clipboard.rs:1011`); RIS and user reset keep the write in flight (`screen.rs:795`, `:801`); committed request may be turned into EBUSY by the event bound (`screen.rs:1772`–`:1776`)
- **Hazards:** no timeout: an abandoned write holds up to `write_limit` (64 MiB) of spool until the next write, commit or terminal drop, and survives RIS; a reused MIME type leaves its earlier bytes in the spool, still counted against `max_size` (`apps/hotaru/crates/hotaru-core/src/kitty_clipboard.rs:519`–`:522` vs `:543`), so rewriting one large type can draw EFBIG although the committed size fits; `wdata`/`walias` are not matched against the write's `id`
- **Reify:** High — state across `Option<WriteState>`, `current` and the decoder carry, five handler paths, clipboard (privilege-adjacent) consequences
- **Related:** Hotaru app `desktop/aizome/apps/hotaru/src/selections.rs:223` (answers reads/writes), `desktop/aizome/apps/hotaru/src/session.rs:448`

<a id="hco-07"></a>

### HCO-07 — Kitty clipboard session password grants and paste-event one-time passwords
- **Where:** `apps/hotaru/crates/hotaru-core/src/kitty_clipboard.rs:630` (`Grants.entries`); also `apps/hotaru/crates/hotaru-core/src/kitty_clipboard.rs:620` (`Grant.read`, `.write`, `.one_time`), `apps/hotaru/crates/hotaru-core/src/screen.rs:63` (`Modes.kitty_paste_events`)
- **Encoding:** FIFO list (max 32) of password grants with per-direction flags and a one-time flag; mode 5522 gates paste events
- **States:** per password: `Ungranted` → `Granted{read?, write?, one_time}` → consumed (one-time) | evicted (33rd grant) | cleared (RIS)
- **Transitions:** user "remember" (`kitty_clipboard.rs:638` via `screen.rs:1784`); paste event mints one-time read grant (`kitty_clipboard.rs:1101`, gated by `screen.rs:1792`); FIFO eviction (`kitty_clipboard.rs:648`–`:650`); one-time removed on any check (`kitty_clipboard.rs:679`–`:681`); read spends a grant only when it asks for data (`kitty_clipboard.rs:1047`–`:1048`); write commit check (`kitty_clipboard.rs:1074`); RIS clears (`screen.rs:796`)
- **Hazards:** eviction ignores `one_time`, so unspent one-time paste grants push out a user's remembered grant; a listing-only read never spends the one-time grant
- **Reify:** High — suspected bug; persistent and one-time grants share one bounded list
- **Suspected bug:** a program enables mode 5522 and, after each paste event, reads only the listing (or nothing); 32 pastes later the password the user chose to remember is evicted by `entries.remove(0)` and the program is prompted again (confidence: medium)
- **Related:** Hotaru app `desktop/aizome/apps/hotaru/src/session.rs:661` (KittyGrant), `desktop/aizome/apps/hotaru/src/session.rs:668` (paste event)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hco-07)

<a id="hco-10"></a>

### HCO-10 — Alternate screen switching (DEC modes 47, 1047, 1049)
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:50` (`Modes.alternate_screen`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:694` (`State.alternate_mode_bits`), `apps/hotaru/crates/hotaru-core/src/screen.rs:689` (`State.primary`, `.alternate`), `apps/hotaru/crates/hotaru-core/src/screen.rs:184` (`Screen.inactive_style`)
- **Encoding:** one bool selects the screen; a 3-bit mask remembers which of 47/1047/1049 were last set (for DECRQM/XTSAVE); each screen keeps its own cursor, saved cursor, margins (copied on switch), semantic state, link and pen-for-erase
- **States:** `Primary` → `Alternate(47|1047|1049)` → `Primary`; invalid but reachable: `alternate_mode_bits` bit 1049 set while on primary (after `CSI ?1049h CSI ?47l`), several bits set at once
- **Transitions:** DECSET/DECRST (`screen.rs:2008`–`:2020`) → `switch_screen_mode` (`screen.rs:1891`–`:1948`): 1049h save cursor (`:1896`), 1047l clear alternate (`:1898`–`:1900`), drop old screen's link (`:1917`), flip (`:1918`), copy cursor/wrap/semantic/margins (`:1920`–`:1934`), 1049h clear and re-place cursor (`:1937`–`:1943`), 1049l restore cursor (`:1944`–`:1946`); RIS → primary (`screen.rs:797`); XTRESTORE replays (`screen.rs:2539`–`:2541`); resize both screens (`screen.rs:927`–`:928`); user clear is a no-op on alternate (`screen.rs:823`–`:825`)
- **Hazards:** deliberately non-idempotent: 1049h while already on alternate saves into the alternate slot and re-clears, and 1049l while on primary still runs `restore_cursor`; the OSC 8 link active at entry is dropped and not restored; a mismatched 47/1049 pair leaves DECRQM reporting a mode that no longer holds
- **Reify:** High — state across a bool, a bitmask and two Screen copies, with side effects at many sites and user-visible results

<a id="hco-12"></a>

### HCO-12 — Pending wrap and soft-wrap row linkage
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:175` (`Screen.wrap_pending`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:118` (`SavedCursor.wrap_pending`), `apps/hotaru/crates/hotaru-core/src/row.rs:222` (`Row.wrapped`, `.wrap_continuation`, `.retained_cols`), `apps/hotaru/crates/hotaru-core/src/screen.rs:696` (`State.previous_char`), `apps/hotaru/crates/hotaru-core/src/screen.rs:702` (`State.autowrap`)
- **Encoding:** DEC "last column" flag per screen; row pair `wrapped` / next row `wrap_continuation` plus spacer-head cell bit for wide glyphs; REP memory of the last printed scalar
- **States:** `Normal` → `PendingWrap` (cursor at right limit − 1) → next print → `SoftWrapped` (row.wrapped, next.wrap_continuation) → `Normal`; invalid but representable: `wrap_pending` with cursor not at the right limit, `wrapped` row whose successor lacks `wrap_continuation`
- **Transitions:** set by print paths (`screen.rs:1123`, `:1260`, `:1350`, `:1460`, `:1567`, `:1623`, `:2362`); consumed by `wrap` (`screen.rs:1138`–`:1154`) marking rows (`:1146`, `:1151`, also `:1219`, `:1231`, `:1319`, `:1327`, `:2336`, `:2339`); cleared by LF (`:360`), CR (`:403`, `:2399`), BS (`:1037`, `:2378`), CUP family (`:1687`), cursor moves (`:2626`), RI (`:2448`), ED/EL/ECH/ICH/DCH/IL/DL (`:2706`, `:2729`, `:2760`, `:2769`, `:2776`, `:2783`, `:2796`, `:2810`), ED 22 (`:2671`), DECALN (`:2127`), clears (`:835`, `:1888`), narrowing grapheme (`:1355`, `:1465`); kept by HT/CBT/scrolling RI; EL unlinks rows (`screen.rs:2740`–`:2748`); saved/restored (`:1853`, `:1877`); copied on screen switch (`:1925`, `:1942`); resize adjustments (`screen.rs:486`–`:497`); reflow re-derives links (`screen.rs:619`–`:620`); row reset/resize clear links (`row.rs:413`–`:414`, `:738`–`:739`); REP memory set (`screen.rs:1126`, `:1201`, `:1573`, `:1638`, `:2317`), read (`screen.rs:1644`)
- **Hazards:** about forty mutation sites keep the cursor-column invariant by convention; on resize the saved cursor drops `wrap_pending` when its row leaves the active area (`:486`–`:488`), but the live cursor does not, so a cursor remapped to (0,0) with a pending wrap lands at (0,1) (`:494`–`:497`); IL/DL/scrolls can separate a `wrapped` row from its continuation; `previous_char` survives every control except RIS
- **Reify:** High — one flag plus row links mutated at ~40 sites with only implicit invariants

<a id="hco-18"></a>

### HCO-18 — Cursor style (DECSCUSR, DEC mode 12, configured default)
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:709` (`State.cursor_blinking`, `.default_cursor_shape`, `.default_cursor_blinking`, `.cursor_is_default`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:29` (`Cursor.shape`, per screen)
- **Encoding:** shape lives in each screen's cursor; blink, the "following the default" flag and the defaults are terminal-wide
- **States:** `FollowingDefault(shape, blink)` → `Explicit(shape, blink)` (DECSCUSR 1–6) → `FollowingDefault` (DECSCUSR 0, DECSTR, RIS); mode 12 overrides blink in either; invalid but reachable: `cursor_is_default` with a screen whose `shape` is not `default_cursor_shape`
- **Transitions:** DECSCUSR (`screen.rs:2479`–`:2481` → `:861`–`:874`, shape written only to the current screen at `:868`); DECSET/RST 12 (`screen.rs:2006`); app default (`screen.rs:853`–`:860`); DECSTR (`screen.rs:2150`); RIS (`screen.rs:802`); 1049 exit keeps primary's own shape (`screen.rs:1920`–`:1924`) and DECRC keeps the current shape (`screen.rs:1871`, `:1874`); snapshot overlays blink (`screen.rs:996`)
- **Hazards:** a default change re-applies only to the active screen; it also overwrites a program's DECSET 12 blink while following the default (`screen.rs:863`–`:864`), contrary to `Terminal::set_default_cursor`'s doc (`hotaru-core/src/lib.rs:163`–`:165`); blink set in the alternate screen leaks to the primary while shape does not
- **Reify:** High — suspected bug; five fields split across terminal and per-screen scope
- **Suspected bug:** the user changes the cursor preference while vim/less runs on the 1049 alternate screen (`desktop/aizome/apps/hotaru/src/session.rs:529` calls `set_default_cursor`); only `alternate.cursor.shape` changes, and on exit the primary keeps the old default shape though `cursor_is_default` is true, until the next DECSCUSR or preference change (confidence: medium)
- **Related:** Hotaru app `desktop/aizome/apps/hotaru/src/session.rs:529`, `desktop/aizome/apps/hotaru/src/app.rs:367`
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#hco-18)

<a id="hco-24"></a>

### HCO-24 — PTY child lifecycle and background reaper (HUP → grace → KILL → wait)
- **Where:** `apps/hotaru/crates/hotaru-io/src/pty.rs:93` (`Pty.master`, `.child`, `.reaper`); also `apps/hotaru/crates/hotaru-io/src/pty.rs:309` (`ExitingChild.deadline`, `.killed`), `apps/hotaru/crates/hotaru-io/src/pty.rs:295` (`REAPER: OnceLock<Result<Sender, String>>`)
- **Encoding:** `Option` master and child on the owner; a process-wide reaper thread with a pending Vec, 250 ms grace deadline and killed flag; reaper creation result cached for the process lifetime
- **States:** `Spawned` → `Running` → (owner drop) `MasterClosed` → `Hangup(deadline)` → `Killed` → `Reaped`; shortcuts: already exited or ECHILD → `Reaped`; reaper `Unstarted` → `Started` | `FailedForever`
- **Transitions:** spawn (`pty.rs:101`–`:197`, reaper first `:106`); owner reap (`pty.rs:249`–`:254`, `:207`); drop closes master and hands off (`pty.rs:276`–`:281`), fallback kill + wait thread (`pty.rs:282`–`:288`); reaper idle block (`pty.rs:328`–`:332`), 10 ms poll (`pty.rs:334`); SIGHUP + pending (`pty.rs:341`–`:350`); reap/ECHILD (`pty.rs:356`–`:359`); SIGKILL after grace (`pty.rs:363`–`:369`)
- **Hazards:** a killed child that never becomes reapable is polled every 10 ms forever; once the session leader has exited, nothing signals the rest of its process group
- **Reify:** High — suspected bug; lifecycle spread across owner, Drop and a polling thread
- **Suspected bug:** `get_or_init` stores an `Err` permanently, so if creating the reaper thread fails once (EAGAIN under RLIMIT_NPROC or memory pressure), every later `Pty::spawn` in that long-lived Hotaru process fails with the cached error after the condition clears (confidence: medium — certain once thread creation fails, which is rare)
- **Related:** Hotaru app `desktop/aizome/apps/hotaru/src/session.rs:479` (spawn), `desktop/aizome/apps/hotaru/src/session.rs:558` (`working_directory`)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hco-24)

<a id="hco-26"></a>

### HCO-26 — Shell prompt hooks: load guards and last-reported-cwd de-duplication (OSC 7)
- **Where:** `apps/hotaru/resources/shell-integration/bash/hotaru.bash:3` (`_hotaru_bash_loaded`, `_hotaru_last_pwd`); also `apps/hotaru/resources/shell-integration/fish/hotaru.fish:4` (`_hotaru_fish_loaded`, `_hotaru_last_pwd`), `apps/hotaru/resources/shell-integration/zsh/hotaru.zsh:7` (`_hotaru_zsh_loaded`, `_hotaru_last_pwd`, `precmd_functions`)
- **Encoding:** unexported shell globals per shell instance; hook registration in `PROMPT_COMMAND`, `fish_prompt`/`PWD` events, `precmd_functions`
- **States:** `Unloaded` → `Loaded+Hooked`; zsh `Loaded+Unhooked` (framework replaced the array) → re-source → `Hooked`; per prompt `Reported(pwd)` → PWD changes → emit OSC 7 → `Reported(new)`
- **Transitions:** load guards (`hotaru.bash:3`–`:4`, `hotaru.fish:4`–`:5`, `hotaru.zsh:7`–`:14`); hook install (`hotaru.bash:27`–`:34`, `hotaru.fish:7`, `hotaru.zsh:34`–`:35`), zsh re-hook (`hotaru.zsh:8`–`:11`); de-duplication and report (`hotaru.bash:9`, `:19`–`:20`; `hotaru.fish:9`, `:14`–`:15`; `hotaru.zsh:21`, `:29`–`:30`)
- **Hazards:** a readonly or associative `PROMPT_COMMAND` silently skips the hook (`hotaru.bash:28`–`:29`); hostnames outside `[a-zA-Z0-9.-]` never report
- **Reify:** High — suspected bug; per-shell copies of the same de-duplication, invisible to the terminal
- **Suspected bug:** the last-PWD cache is never invalidated when something else reports OSC 7. In `~/src`, run a nested integrated shell (or a program that emits OSC 7), `cd /tmp`, then exit. The outer prompt's PWD equals `_hotaru_last_pwd`, so it stays silent, and Hotaru keeps `/tmp` as the pane's directory and title for new tabs (confidence: medium)
- **Related:** Hotaru app `desktop/aizome/apps/hotaru/src/session.rs:423`–`:434` (accepts OSC 7 via `apps/hotaru/crates/hotaru-io/src/launch.rs:142`)
- **Reproduction:** Confirmed (integration): [summary](reproductions.md#hco-26)
