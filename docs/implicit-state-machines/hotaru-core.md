# HCO: Hotaru terminal engine

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `apps/hotaru/crates`. Audited at `8e0acbe5`; line numbers refer to that commit.

41 files, 0 pending. 26 implicit machines: 7 High, 8 Medium, 11 Low.

## Machines

<a id="hco-01"></a>

### HCO-01 — Copy formatter open-scope emitter (style, link, deferred blanks)
- **Where:** `apps/hotaru/crates/hotaru-core/src/formatter.rs:444` (`render` locals `style`, `link`, `blank_rows`, `blanks`); also `apps/hotaru/crates/hotaru-core/src/formatter.rs:86` (`Output.html_tags`)
- **Encoding:** per-call locals: current open `Style` (non-default means an SGR/`<div>` is open), `Option<LinkRef>` (open OSC 8 / `<a>`), deferred newline and space counters; HTML tag stack of `(HtmlTag, opening bytes)` for re-opening crossed scopes
- **States:** per row/cell: `Plain` ⇄ `StyleOpen` ⇄ `LinkOpen` (both may be open, in either nesting order); blanks `None` → `Deferred(n)` → flushed; terminal: everything closed
- **Transitions:** blank-row flush closes style (`formatter.rs:484`); VT blanks close style+link (`formatter.rs:526`, `:530`); style change close/open (`formatter.rs:537`–`:549`); link change close/open (`formatter.rs:558`–`:565`); crossed HTML scopes close-and-reopen (`formatter.rs:101`–`:114`); final close (`formatter.rs:589`–`:594`); cancellation/limit abort in `Output.put` (`formatter.rs:116`, `:119`)
- **Hazards:** none observed; single call, bounded by `max_bytes`, cancel checked on every write
- **Reify:** Low — local to one function, no state survives the call

<a id="hco-02"></a>

### HCO-02 — Scrollback block lifecycle and incremental compression cursor
- **Where:** `apps/hotaru/crates/hotaru-core/src/history.rs:306` (`Entry.block`, `.skip`, `.size_bound`, `.attempted`); also `apps/hotaru/crates/hotaru-core/src/history.rs:319` (`History.scan`, `.scan_rows`, `.len`, `.logical_bytes`), `apps/hotaru/crates/hotaru-core/src/history.rs:62` (`enum Storage`)
- **Encoding:** partially explicit: `Storage::{Resident, Compressed}` enum plus per-entry `attempted` (sealed) flag, `skip` (evicted prefix), `size_bound`; history-wide `scan` block cursor and `scan_rows` row count of blocks already inspected
- **States:** block `Open(Resident, !attempted, <64 rows, ≤64 KiB)` → `Sealed(Resident, attempted or full)` → `Compressed` → prefix-evicted (`skip>0`) → removed; cursor `scan` ∈ 0..=blocks.len(); invariant (implicit): every block before `scan` has `attempted == true` and `scan_rows == Σ len(blocks[..scan])`
- **Transitions:** append to open block (`history.rs:364`–`:377`); open new block (`history.rs:379`–`:388`); seal + compress (`history.rs:495`–`:500`); evict row, advance `skip`, adjust `scan_rows` (`history.rs:437`–`:441`); drop empty block, decrement `scan` (`history.rs:443`–`:447`); `clear` resets all (`history.rs:352`–`:360`); `from_rows` rebuilds unsealed resident blocks (`history.rs:457`–`:463`); driven by `compress_history_step` (`screen.rs:934`–`:938`) and `prune`/`pop_front_recycle` from `scroll_up` (`screen.rs:319`–`:325`)
- **Hazards:** the scan invariant is maintained by three functions without a single owner; every primary resize decodes all compressed blocks (`into_rows`, `history.rs:476`–`:477`, via `screen.rs:440`) and rebuilds them resident and unsealed, so a drag-resize repeatedly inflates history to its full logical size on the PTY worker and redoes compression; a shared resident block is copied on append (`history.rs:371`)
- **Reify:** Medium — enum exists, but open/sealed and the scan cursor are flags/counters with cross-function invariants

<a id="hco-03"></a>

### HCO-03 — Content identity tokens for asynchronous readers (epoch, origin, generation)
- **Where:** `apps/hotaru/crates/hotaru-core/src/history.rs:320` (`History.epoch`, `.origin`); also `apps/hotaru/crates/hotaru-core/src/position.rs:7` (`ContentEpoch`), `apps/hotaru/crates/hotaru-core/src/position.rs:22` (`ContentPosition`), `apps/hotaru/crates/hotaru-core/src/screen.rs:727` (`State.generation`), `apps/hotaru/crates/hotaru-core/src/content.rs:34` (`ContentSnapshot.generation`), `apps/hotaru/crates/hotaru-core/src/search.rs:55` (`Results.generation_sensitive_prefix`, `.multiline`), `apps/hotaru/crates/hotaru-core/src/search.rs:176` (`Matcher.generation_sensitive`)
- **Encoding:** `Arc<()>` identity per history lifetime, evicted-row counter `origin`, terminal-wide `generation: u64`; search results carry the captured triple plus a latch marking matches that depend on active rows
- **States:** per capture: `Valid` → `HistoryValidOnly` (generation changed) → `Stale` (epoch changed, or origin changed for multiline matches); `generation_sensitive` latch `false` → `true` once rows reach the active screen
- **Transitions:** new epoch on `History::clear` (`history.rs:353`; from ED 3 `screen.rs:2665`, user clear `screen.rs:829`, alt clear `screen.rs:1884`), on origin overflow (`history.rs:452`), on resize via `History::from_rows` (`screen.rs:499`), on RIS via `Self::new` (`screen.rs:797`); `origin += 1` per evicted row (`history.rs:448`); generation bump per feed (`hotaru-core/src/lib.rs:251`), RIS (`screen.rs:798`), user clear (`screen.rs:850`), defaults (`screen.rs:859`, `:887`, `:902`), resize (`screen.rs:929`); latch set (`search.rs:341`–`:345`, `:429`); validation (`search.rs:77`, `:80`, `:87`–`:92`)
- **Hazards:** any resize, even height-only without reflow, mints a new epoch and invalidates all retained Find results; generation bumps on every `feed_inner` call even when nothing changed, so active-row matches die on any PTY read; alternate history epoch is reused across DECSET 47 sessions (cleared only at 1049 entry/1047 exit) and is protected only by generation
- **Reify:** Medium — three tokens, minted at ten sites, validated in a different crate's worker
- **Related:** Hotaru app `desktop/aizome/apps/hotaru/src/search.rs:130`, `desktop/aizome/apps/hotaru/src/app.rs:769`, `desktop/aizome/apps/hotaru/src/session.rs:539` (consumers of `resolve_match`)

<a id="hco-04"></a>

### HCO-04 — Kitty keyboard progressive-enhancement flag stack (per screen)
- **Where:** `apps/hotaru/crates/hotaru-core/src/input.rs:146` (`KittyKeyboard.flags`, `.index`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:180` (`Screen.kitty_keyboard`), `apps/hotaru/crates/hotaru-core/src/screen.rs:56` (`Modes.kitty_flags`)
- **Encoding:** 8-slot ring of `KittyFlags` with a top index, one per screen; `State.modes.kitty_flags` is a dead copy (always 0) that readers overlay with the active screen's `current()` (`screen.rs:963`, `:971`, `:1007`)
- **States:** stack depth 0..8 (ring), top flags 0..31; separate for primary and alternate
- **Transitions:** `CSI > u` push (`screen.rs:2511`–`:2518` → `input.rs:154`); `CSI < u` pop (`screen.rs:2519` → `input.rs:158`); `CSI = u` set/or/clear (`screen.rs:2524`–`:2529` → `input.rs:168`); RIS reset via `State::new` (`screen.rs:797`)
- **Hazards:** a ninth push silently overwrites the oldest; neither DECSTR nor 1049 entry resets the alternate stack, so flags left by a crashed full-screen program carry into the next alternate-screen session; the dead `Modes.kitty_flags` copy is a trap for any internal reader
- **Reify:** Low — transitions centralised in three methods

<a id="hco-05"></a>

### HCO-05 — Mouse report motion de-duplication
- **Where:** `apps/hotaru/crates/hotaru-core/src/input.rs:724` (`MouseEncoder.last_cell`, `.configuration`)
- **Encoding:** last reported cell and the (tracking, format, geometry) it was computed under
- **States:** `NoLastCell` → `LastCell(x,y)`; reset to `NoLastCell` on configuration change
- **Transitions:** configuration change clears (`input.rs:741`–`:744`); every reported event records the cell (`input.rs:810`); motion into the same cell suppressed (`input.rs:804`–`:809`); `reset()` (`input.rs:729`–`:731`)
- **Hazards:** button-held state is the caller's (`any_button_pressed` argument), so the drag phase lives outside this object; press/release also update `last_cell`, suppressing a motion report in the pressed cell (reference behaviour)
- **Reify:** Low — two fields, one method
- **Related:** Hotaru app `desktop/aizome/apps/hotaru/src/input.rs:222`, `desktop/aizome/apps/hotaru/src/session.rs:503`

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

<a id="hco-08"></a>

### HCO-08 — History byte-budget signal (dirty/peak/pause accounting)
- **Where:** `apps/hotaru/crates/hotaru-core/src/memory.rs:29` (`BudgetSignal.bytes`, `.peak`, `.dirty`, `.paused`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:168` (`Screen.byte_limit`, `.history_budget`, `.budget_signal`), `apps/hotaru/crates/hotaru-core/src/row.rs:230` (`Row.memory_charge`), `apps/hotaru/crates/hotaru-core/src/memory.rs:85` (`Charge`)
- **Encoding:** atomics for current active bytes, high-water mark since last take, dirty and paused flags; each active row's `Charge` attached to the signal; `history_budget` cached from the last applied change
- **States:** `Clean` → `Dirty(peak)` (any row charge change) → `Applied` (history pruned to `limit − peak`, budget recomputed); `Paused` (adjustments dropped) → `Resumed`
- **Transitions:** `adjust` marks dirty and raises peak (`memory.rs:48`–`:66`, from `Charge::update` `memory.rs:106`–`:112` via `row.rs:267`–`:270`); `take_change` (`memory.rs:73`–`:82`) in `apply_budget_change` (`screen.rs:258`–`:267`); checked after every parsed byte (`hotaru-core/src/lib.rs:247`, `screen.rs:951`–`:954`) and in scroll loops (`screen.rs:310`, `:342`); pause/resume around recycle (`screen.rs:334`, `:339`) and resize (`screen.rs:436`, ended by `rebase` `memory.rs:42`–`:47` at `screen.rs:508`); explicit charge transfer (`screen.rs:340`); link charge (`screen.rs:240`–`:248`)
- **Hazards:** pause/resume is not a guard: a panic or early return between them silently drops all adjustments until the next resize rebase; `history_budget` is stale between applications; correctness assumes a single writer even though readers may fill `Charge` caches from other threads (`memory.rs:113`–`:121`)
- **Reify:** Medium — dirty/peak/pause spread over one struct plus three Screen fields and many call sites

<a id="hco-09"></a>

### HCO-09 — VT/DEC escape-sequence parser
- **Where:** `apps/hotaru/crates/hotaru-core/src/parser.rs:43` (`enum State`); also `apps/hotaru/crates/hotaru-core/src/parser.rs:65` (`Parser.param_count`, `.has_parameters`, `.private`, `.intermediate_count`, `.osc`, `.osc_overflow`, `.osc_body`, `.osc_limit`, `.dcs_final`, `.utf8_value`, `.utf8_left`, `.utf8_min`), `apps/hotaru/crates/hotaru-core/src/lib.rs:120` (`Terminal.parser`)
- **Encoding:** partially explicit: explicit 13-state enum with one `advance` dispatcher; beside it a UTF-8 sub-decoder (`utf8_left > 0` only meaningful in `Ground`), OSC capture phase (`osc_body` after the command's `;`, per-command `osc_limit`, `osc_overflow` discard-until-ST), and header storage
- **States:** `Ground`(+`Utf8Pending(n)`) → `Escape` → `Csi`|`DcsEntry`→`DcsString`→`DcsEscape`|`Osc`→`OscEscape`|`SosPmApcString`|`IgnoreEscape`|`IgnoreCsi`|`IgnoreString`→`IgnoreStringEscape` → `Ground`; overflowed capture is a hidden sub-state of `Osc`/`DcsString`
- **Transitions:** UTF-8 continuation/rejection (`parser.rs:89`–`:118`); CAN/SUB abort (`parser.rs:119`–`:123`); ESC entry (`parser.rs:329`–`:332`); CSI/DCS entry (`parser.rs:333`–`:340`); OSC entry (`parser.rs:341`–`:347`); C1 anywhere (`parser.rs:353`–`:365`); OSC limit selection (`parser.rs:253`–`:265`); overflow (`parser.rs:374`–`:377`, `:384`–`:391`); dispatch to Ground (`parser.rs:155`, `:234`, `:301`, `:415`); invalid header → ignore (`parser.rs:396`–`:404`); re-dispatch after ESC in a string (`parser.rs:273`–`:275`, `:303`–`:305`, `:322`–`:323`); user reset replaces the parser (`hotaru-core/src/lib.rs:177`); ground fast path bypasses `advance` (`hotaru-core/src/lib.rs:219`–`:245`)
- **Hazards:** `osc` is only ever `clear()`ed, never shrunk, so one 8 MiB OSC 52/5522 leaves that capacity allocated for the terminal's lifetime, outside the history budget; correctness of `params[param_count - 1]` relies on `enter_parameters` setting `param_count = 1`
- **Reify:** Low — already an explicit enum with one transition function; only the UTF-8 and OSC-capture sub-state sit beside it

<a id="hco-10"></a>

### HCO-10 — Alternate screen switching (DEC modes 47, 1047, 1049)
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:50` (`Modes.alternate_screen`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:694` (`State.alternate_mode_bits`), `apps/hotaru/crates/hotaru-core/src/screen.rs:689` (`State.primary`, `.alternate`), `apps/hotaru/crates/hotaru-core/src/screen.rs:184` (`Screen.inactive_style`)
- **Encoding:** one bool selects the screen; a 3-bit mask remembers which of 47/1047/1049 were last set (for DECRQM/XTSAVE); each screen keeps its own cursor, saved cursor, margins (copied on switch), semantic state, link and pen-for-erase
- **States:** `Primary` → `Alternate(47|1047|1049)` → `Primary`; invalid but reachable: `alternate_mode_bits` bit 1049 set while on primary (after `CSI ?1049h CSI ?47l`), several bits set at once
- **Transitions:** DECSET/DECRST (`screen.rs:2008`–`:2020`) → `switch_screen_mode` (`screen.rs:1891`–`:1948`): 1049h save cursor (`:1896`), 1047l clear alternate (`:1898`–`:1900`), drop old screen's link (`:1917`), flip (`:1918`), copy cursor/wrap/semantic/margins (`:1920`–`:1934`), 1049h clear and re-place cursor (`:1937`–`:1943`), 1049l restore cursor (`:1944`–`:1946`); RIS → primary (`screen.rs:797`); XTRESTORE replays (`screen.rs:2539`–`:2541`); resize both screens (`screen.rs:927`–`:928`); user clear is a no-op on alternate (`screen.rs:823`–`:825`)
- **Hazards:** deliberately non-idempotent: 1049h while already on alternate saves into the alternate slot and re-clears, and 1049l while on primary still runs `restore_cursor`; the OSC 8 link active at entry is dropped and not restored; a mismatched 47/1049 pair leaves DECRQM reporting a mode that no longer holds
- **Reify:** High — state across a bool, a bitmask and two Screen copies, with side effects at many sites and user-visible results

<a id="hco-11"></a>

### HCO-11 — Saved cursor (DECSC/DECRC, SCOSC/SCORC, 1048, 1049)
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:173` (`Screen.saved`, `.saved_valid`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:114` (`SavedCursor`), `apps/hotaru/crates/hotaru-core/src/screen.rs:695` (`State.save_cursor_mode`)
- **Encoding:** per-screen slot (cursor, pen, origin, pending wrap, charsets, protection) plus a validity bool used only for resize pinning; a separate DECRQM bit for 1048
- **States:** `Empty(default slot)` → `Saved` → (restore) `Saved`; `saved_valid == false` still restores `SavedCursor::default()` (home, default pen)
- **Transitions:** save (`screen.rs:1848`–`:1860`) from ESC 7 (`:2427`), `CSI s` without DECLRMM (`:2836`–`:2837`), 1048h (`:2024`), 1049h (`:1896`); restore (`screen.rs:1861`–`:1878`) from ESC 8 (`:2428`), `CSI u` (`:2840`), 1048l (`:2026`), 1049l (`:1945`); DECSTR invalidates (`screen.rs:2157`–`:2158`); resize re-pins (`screen.rs:445`–`:447`, `:484`–`:493`)
- **Hazards:** with DECLRMM on, `CSI s` is DECSLRM, so a program's SCOSC is silently lost; restore ignores the saved `visible`/`shape` fields (`screen.rs:1870`–`:1874`); `save_cursor_mode` is independent of which call filled the slot; XTRESTORE of 1048 performs a real save/restore
- **Reify:** Medium — small slot, but eight call sites and per-screen scoping

<a id="hco-12"></a>

### HCO-12 — Pending wrap and soft-wrap row linkage
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:175` (`Screen.wrap_pending`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:118` (`SavedCursor.wrap_pending`), `apps/hotaru/crates/hotaru-core/src/row.rs:222` (`Row.wrapped`, `.wrap_continuation`, `.retained_cols`), `apps/hotaru/crates/hotaru-core/src/screen.rs:696` (`State.previous_char`), `apps/hotaru/crates/hotaru-core/src/screen.rs:702` (`State.autowrap`)
- **Encoding:** DEC "last column" flag per screen; row pair `wrapped` / next row `wrap_continuation` plus spacer-head cell bit for wide glyphs; REP memory of the last printed scalar
- **States:** `Normal` → `PendingWrap` (cursor at right limit − 1) → next print → `SoftWrapped` (row.wrapped, next.wrap_continuation) → `Normal`; invalid but representable: `wrap_pending` with cursor not at the right limit, `wrapped` row whose successor lacks `wrap_continuation`
- **Transitions:** set by print paths (`screen.rs:1123`, `:1260`, `:1350`, `:1460`, `:1567`, `:1623`, `:2362`); consumed by `wrap` (`screen.rs:1138`–`:1154`) marking rows (`:1146`, `:1151`, also `:1219`, `:1231`, `:1319`, `:1327`, `:2336`, `:2339`); cleared by LF (`:360`), CR (`:403`, `:2399`), BS (`:1037`, `:2378`), CUP family (`:1687`), cursor moves (`:2626`), RI (`:2448`), ED/EL/ECH/ICH/DCH/IL/DL (`:2706`, `:2729`, `:2760`, `:2769`, `:2776`, `:2783`, `:2796`, `:2810`), ED 22 (`:2671`), DECALN (`:2127`), clears (`:835`, `:1888`), narrowing grapheme (`:1355`, `:1465`); kept by HT/CBT/scrolling RI; EL unlinks rows (`screen.rs:2740`–`:2748`); saved/restored (`:1853`, `:1877`); copied on screen switch (`:1925`, `:1942`); resize adjustments (`screen.rs:486`–`:497`); reflow re-derives links (`screen.rs:619`–`:620`); row reset/resize clear links (`row.rs:413`–`:414`, `:738`–`:739`); REP memory set (`screen.rs:1126`, `:1201`, `:1573`, `:1638`, `:2317`), read (`screen.rs:1644`)
- **Hazards:** about forty mutation sites keep the cursor-column invariant by convention; on resize the saved cursor drops `wrap_pending` when its row leaves the active area (`:486`–`:488`), but the live cursor does not, so a cursor remapped to (0,0) with a pending wrap lands at (0,1) (`:494`–`:497`); IL/DL/scrolls can separate a `wrapped` row from its continuation; `previous_char` survives every control except RIS
- **Reify:** High — one flag plus row links mutated at ~40 sites with only implicit invariants

<a id="hco-13"></a>

### HCO-13 — Scroll region and left/right margins
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:176` (`Screen.top`, `.bottom`, `.left`, `.right`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:701` (`State.left_right_margin_mode`)
- **Encoding:** four per-screen indices plus a terminal-wide DECLRMM flag; documented as shared terminal state but stored per screen and copied on switch
- **States:** `FullScreen` → `Region(top,bottom)` and/or `Columns(left,right)` (only with DECLRMM) → `FullScreen`; implicit invariant: `!left_right_margin_mode ⇒ full_width()` (debug-asserted at `screen.rs:2372`)
- **Transitions:** DECSTBM (`screen.rs:2815`–`:2824`); DECSLRM (`screen.rs:2826`–`:2835`); DECRST 69 (`screen.rs:1962`–`:1969`); DECSTR (`screen.rs:2153`–`:2156`); DECALN (`screen.rs:2121`–`:2124`); resize (`screen.rs:504`–`:507`); copy on screen switch (`screen.rs:1913`–`:1916`, `:1931`–`:1934`); temporary override-and-restore in `scroll_clear` (`screen.rs:290`–`:296`), user clear (`screen.rs:840`–`:846`), IL/DL (`screen.rs:2801`–`:2808`)
- **Hazards:** the per-screen copies agree only because every switch copies; the temporary overrides restore by assignment, so any early return between them would leak a full-screen region; print and C0 fast paths branch on DECLRMM (`screen.rs:1073`, `:2270`, `:2368`) and rely on the invariant
- **Reify:** Medium — five fields, ten mutation sites, one cross-screen invariant

<a id="hco-14"></a>

### HCO-14 — Active OSC 8 hyperlink
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:181` (`Screen.active_link`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:713` (`State.next_link_id`)
- **Encoding:** `Option<LinkRef>` per screen, charged to the budget; implicit-id counter
- **States:** `NoLink` → `Linking(link)` → `NoLink`
- **Transitions:** OSC 8 open (`screen.rs:2958`–`:2969`); close only when URI and id are both empty (`screen.rs:2953`–`:2956`); budget-charged setter (`screen.rs:240`–`:248`); dropped on leaving a screen (`screen.rs:1917`); RIS (`screen.rs:797`); applied at every print path (`screen.rs:1114`, `:1254`, `:1563`, `:1617`, `:2356`)
- **Hazards:** a program that dies mid-link keeps linking all later output; links do not return after an alternate-screen round trip; `OSC 8;id=x;` keeps the link open
- **Reify:** Low — one Option, few writers

<a id="hco-15"></a>

### HCO-15 — OSC 133 semantic prompt state
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:185` (`Screen.semantic`, `.semantic_clear_eol`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:717` (`State.semantic_markers_seen`), `apps/hotaru/crates/hotaru-core/src/row.rs:224` (`Row.prompt`), `apps/hotaru/crates/hotaru-core/src/row.rs:196` (cell semantic bits)
- **Encoding:** current content class per screen, an "input until end of line" flag, a terminal-wide one-way latch, row prompt marks and per-cell classes retained into scrollback
- **States:** `Output` → `Prompt` (A/N/P) → `Input` (B) | `InputToEol` (I) → `Output` (C/D, or explicit LF after I); latch `Unseen` → `Seen` (until RIS)
- **Transitions:** `semantic_prompt` (`screen.rs:1800`–`:1847`: latch `:1809`, fresh line `:1818`–`:1824`, class `:1826`, EOL flag `:1832`, row marks `:1839`–`:1845`); LF ends I and marks continuation rows (`screen.rs:366`–`:371`); soft wrap preserves (`screen.rs:375`–`:382`); screen switch copies except on 1049 exit (`screen.rs:1926`–`:1929`); consumers: user clear (`screen.rs:826`–`:827`), prompt-preserving ED 2 (`screen.rs:2678`–`:2689`)
- **Hazards:** the latch and row marks outlive the shell that produced them (e.g. after `exec` into an unintegrated shell), so ED 2 and Clear Screen may still treat rows as prompts; 47/1047 exits carry the alternate screen's class into the primary
- **Reify:** Medium — four encodings, transitions in three functions

<a id="hco-16"></a>

### HCO-16 — DEC/ANSI terminal mode flags, XTSAVE/XTRESTORE and soft reset
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:46` (`Modes`: application_cursor, bracketed_paste, focus_events, application_keypad, backarrow, ignore_keypad_with_numlock, alt_esc_prefix, modify_other_keys, mouse_tracking, mouse_format, mouse_alternate_scroll, grapheme_cluster, kitty_paste_events); also `apps/hotaru/crates/hotaru-core/src/screen.rs:691` (`State.modes`, `.default_grapheme_cluster`, `.saved_modes`, `.mouse_tracking_modes`, `.mouse_format_modes`, `.origin`, `.autowrap`, `.insert`, `.newline`, `.protected`), `apps/hotaru/crates/hotaru-core/src/screen.rs:138` (`SavedModes`)
- **Encoding:** ~25 independent bools/enums; bitmasks remembering which mouse tracking/format modes are set; one XTSAVE slot per mode in a u32
- **States:** each mode `Reset` ⇄ `Set`; invalid but reachable: `mouse_tracking_modes` bit for 1000 set while `mouse_tracking == None` (after `?1000h ?1002h ?1002l`), likewise formats
- **Transitions:** `mode()` (`screen.rs:1949`–`:2035`); DECRQM (`screen.rs:2036`–`:2108`, `:2482`–`:2486`); XTSAVE/XTRESTORE (`screen.rs:2534`–`:2543`); DECSTR (`screen.rs:2137`–`:2161`); DECKPAM/DECKPNM (`screen.rs:2457`–`:2458`); modifyOtherKeys (`screen.rs:2545`–`:2555`); DECSCA (`screen.rs:2476`–`:2478`); RIS (`screen.rs:797`–`:804`); app default 2027 (`screen.rs:883`–`:888`)
- **Hazards:** XTRESTORE replays through `mode()` with side effects (6 homes the cursor, 1048 restores, 47/1047/1049 switch screens); restoring several mouse modes collapses to the last one restored, so saved `{1000: on, 1002: off}` restores to no tracking; DECRQM can report a tracking mode as set while no events are sent
- **Reify:** Medium — many fields, but each mode is local and the invalid combinations follow the reference terminal

<a id="hco-17"></a>

### HCO-17 — Character set designation and shifts (G0–G3, SI/SO, LS2/LS3, SS2/SS3)
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:705` (`State.charset`, `.active_charset`, `.single_shift`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:119` (`SavedCursor.charset`, `.active_charset`)
- **Encoding:** four u8 designations (0 ASCII, 1 DEC graphics, 2 UK), active index 0..3, one-shot `Option<usize>`
- **States:** `GL = Gn` (n 0..3) × `SingleShift(None | G2 | G3)`
- **Transitions:** designate (`screen.rs:2411`–`:2417`); SO/SI (`screen.rs:2401`–`:2402`, `:1064`–`:1065`); SS2/SS3 (`screen.rs:2423`–`:2424`); LS2/LS3 (`screen.rs:2425`–`:2426`); single shift consumed per printed scalar (`screen.rs:2280`, `:1164`); DECRC restores and drops single shift (`screen.rs:1865`–`:1867`); DECSTR (`screen.rs:2147`–`:2149`); fast-path gate (`screen.rs:1071`–`:1072`, `:1476`–`:1477`)
- **Hazards:** none observed; magic u8 codes rather than an enum
- **Reify:** Low — small and local

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

<a id="hco-19"></a>

### HCO-19 — Dynamic color overrides (OSC 4/10/11 vs configured defaults)
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:719` (`State.default_colors`, `.palette`, `.palette_overrides`, `.foreground`, `.background`, `.foreground_overridden`, `.background_overridden`)
- **Encoding:** effective colors plus parallel override flags deciding whether configured defaults may replace them
- **States:** per slot `Default` ⇄ `Overridden`
- **Transitions:** OSC 4 (`screen.rs:3009`–`:3011`); OSC 10/11 (`screen.rs:3020`–`:3023`, `:3028`–`:3031`); OSC 104/110/111 (`screen.rs:3033`–`:3051`); defaults respect overrides (`screen.rs:889`–`:903`); DECSTR resets the palette only (`screen.rs:2159`–`:2160`); RIS resets the palette but keeps fg/bg overrides (`screen.rs:788`–`:789`, `:805`–`:815`)
- **Hazards:** value and flag arrays must change together at every site; DECSTR/RIS treat palette and fg/bg differently by design
- **Reify:** Low — per-slot two-state flags confined to OSC handlers and resets

<a id="hco-20"></a>

### HCO-20 — Pending reply/event queues and per-feed clipboard bound
- **Where:** `apps/hotaru/crates/hotaru-core/src/screen.rs:728` (`State.responses`, `.events`); also `apps/hotaru/crates/hotaru-core/src/screen.rs:135` (`MAX_CLIPBOARD_EVENTS`, `MAX_CLIPBOARD_BYTES`), `apps/hotaru/crates/hotaru-core/src/lib.rs:216` (reply budget)
- **Encoding:** two Vecs drained by the embedder; coalescing of UI events; clipboard requests counted from what is still queued
- **States:** `Empty` → `Pending` → `ReplyBudgetReached` (feed stops) | `ClipboardPrefixFull` (OSC 52 writes dropped, reads answered empty, Kitty requests EBUSY) → drained → `Empty`
- **Transitions:** reply (`screen.rs:1689`–`:1691`); event with coalescing (`screen.rs:1692`–`:1711`); OSC 52 bound (`screen.rs:1718`–`:1733`); Kitty bound (`screen.rs:1772`–`:1776`); feed stops at budget (`hotaru-core/src/lib.rs:216`–`:218`); drain (`hotaru-core/src/lib.rs:298`, `:301`); RIS keeps queued items (`screen.rs:793`–`:794`, `:799`–`:800`)
- **Hazards:** the clipboard bound is only per feed if the embedder drains after every bounded feed; an OSC 52 `?` read that fails allocation (`screen.rs:1728`–`:1733`) gets no answer; two OSC 9/777 notifications in one undrained window keep only the last (the app also keeps one per drain)
- **Reify:** Medium — contract split between core and embedder
- **Related:** Hotaru app `desktop/aizome/apps/hotaru/src/session.rs:416` (drain), `desktop/aizome/apps/hotaru/src/session.rs:718` (`feed_bounded`)

<a id="hco-21"></a>

### HCO-21 — Nonblocking clipboard pipe transfer (writer and reader)
- **Where:** `apps/hotaru/crates/hotaru-io/src/clipboard.rs:43` (`Transfer.fd`, `.cancellation`, `.deadline`, `.state`); also `apps/hotaru/crates/hotaru-io/src/clipboard.rs:89` (`ClipboardWriter.payload`, `.written`, `.length`), `apps/hotaru/crates/hotaru-io/src/clipboard.rs:218` (`ClipboardReader.bytes`, `.maximum`), `apps/hotaru/crates/hotaru-io/src/clipboard.rs:33` (`enum TransferState`)
- **Encoding:** partially explicit: `TransferState` enum, but `fd: Option` and `payload: Option` duplicate it (Some ⇔ Pending); cancellation is a shared AtomicBool and the deadline an `Instant`, both observed only inside `pump`
- **States:** `Pending` → `Complete` | `Cancelled` | `TimedOut` | `ReceiverClosed` | `LimitExceeded` | `Failed`
- **Transitions:** lazy cancel/timeout (`clipboard.rs:73`–`:83`); writer complete (`clipboard.rs:143`–`:145`, `:205`–`:207`), EAGAIN stays pending (`:186`–`:188`), EPIPE (`:189`–`:192`), errors (`:193`, `:196`–`:202`, `:152`); reader EOF (`clipboard.rs:297`–`:299`), cap (`:301`–`:303`), errors (`:294`, `:304`–`:310`); terminal step drops fd/payload (`clipboard.rs:67`–`:71`, `:129`–`:132`)
- **Hazards:** a caller that stops pumping after the deadline keeps the fd open; reader retains partial bytes after `LimitExceeded`/`TimedOut` by design; no consumer under `desktop/` at this commit (only `apps/hotaru/crates/hotaru-io/tests/clipboard.rs`)
- **Reify:** Low — already mostly explicit; only the Option fields shadow the enum

<a id="hco-22"></a>

### HCO-22 — Thread-scoped SIGPIPE block guard
- **Where:** `apps/hotaru/crates/hotaru-io/src/fd.rs:29` (`BlockSigpipe.set`, `.previous`, `.already_pending`)
- **Encoding:** RAII guard holding the previous thread mask and whether SIGPIPE was already pending
- **States:** `Blocked(already_pending?)` → `Consumed` (optional) → `Restored` (drop)
- **Transitions:** block (`fd.rs:45`); consume a newly generated signal (`fd.rs:63`–`:78`); restore on drop (`fd.rs:81`–`:88`); used per writer pump (`clipboard.rs:149`, `:190`)
- **Hazards:** if SIGPIPE was already pending, the new one merges and stays pending after the mask is restored (harmless under Rust's default SIG_IGN)
- **Reify:** Low — scoped guard

<a id="hco-23"></a>

### HCO-23 — Default-shell integration bootstrap handoff (bash rcfile, zsh ZDOTDIR swap, fish init command)
- **Where:** `apps/hotaru/crates/hotaru-io/src/launch.rs:19` (`prepare_with`); also `apps/hotaru/crates/hotaru-io/src/launch.rs:78` (`HOTARU_ZSH_ZDOTDIR`, `HOTARU_ZSH_ZDOTDIR_SET`, `ZDOTDIR` env handoff), `apps/hotaru/resources/shell-integration/bash/rc.bash:4`, `apps/hotaru/resources/shell-integration/zsh/hotaru.zsh:3`, `apps/hotaru/resources/shell-integration/zsh/.zshenv:4`
- **Encoding:** decision made before fork from `integrate_shell`, empty args, `HOTARU_RESOURCES_DIR` and script existence; state passed to the child through argv and environment variables
- **States:** `NotIntegrated` (explicit args, flag off, no resources, missing script) | `Armed` (args/env set at spawn) → `Bootstrapped` (shell restores the user's ZDOTDIR / sources `~/.bashrc`) → `Hooked`
- **Transitions:** guards (`launch.rs:47`–`:56`, `:66`–`:68`); bash `--rcfile` (`launch.rs:73`–`:75`) → `rc.bash:5`–`:10`; zsh env swap (`launch.rs:78`–`:87`) → zsh `.zshenv` restores or unsets ZDOTDIR and sources the user's `.zshenv` (`zsh/.zshenv:4`–`:12`) → `hotaru.zsh` if interactive (`zsh/.zshenv:13`–`:15`); fish `--init-command` (`launch.rs:89`–`:92`) → `hotaru.fish`; terminfo selection (`launch.rs:32`–`:46`)
- **Hazards:** if `/etc/zshenv` sets ZDOTDIR, zsh never reads Hotaru's `.zshenv`, so integration is skipped silently and the `HOTARU_ZSH_*` variables leak into the session; script existence is checked once before fork
- **Reify:** Low — one-shot handoff
- **Related:** Hotaru app `desktop/aizome/apps/hotaru/src/session.rs:479` (spawn)

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

<a id="hco-25"></a>

### HCO-25 — (test harness) Kitty clipboard embedder model replay
- **Where:** `apps/hotaru/crates/hotaru-replay/src/bin/kitty-clipboard-replay.rs:65` (`Scenario.clipboards`, `.reads`, `.writes`, `.entropy`, `.entropy_at`)
- **Encoding:** per-scenario clipboards, FIFO decision queues for reads and writes, entropy cursor for one-time passwords
- **States:** per step `Feed` → `Drain` (answer each request from the queues, update clipboards/grants) | `Paste` (draw entropy → paste event) → next step
- **Transitions:** feed and drain per chunk (`kitty-clipboard-replay.rs:121`–`:125`); read decisions (`:184`–`:205`), grants on remember (`:202`–`:204`, `:220`–`:222`); write decisions (`:213`–`:224`); entropy draw (`:154`–`:169`); paste (`:138`–`:145`)
- **Hazards:** exhausted decision queues default to success, masking under-specified scenarios; exhausted entropy silently reports `written: false`
- **Reify:** Low — harness

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

## Already explicit

- `apps/hotaru/crates/hotaru-core/src/unicode.rs:39` `struct BreakState(u8)` — packed grapheme-break state (after-linker bit plus default/emoji/RI base); transitions centralised in `fn transition` (`apps/hotaru/crates/hotaru-core/src/unicode.rs:122`) compiled into `BREAKS` and applied only by `fn break_properties` (`apps/hotaru/crates/hotaru-core/src/unicode.rs:47`); callers rebuild it from the cell's cluster on every print (`apps/hotaru/crates/hotaru-core/src/screen.rs:1289`–`:1297`), so nothing persists between calls

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `apps/hotaru/crates/hotaru-core/src/content.rs` | 144 | machines | [HCO-03](#hco-03) | immutable capture carrying position and generation tokens; cancel checks |
| `apps/hotaru/crates/hotaru-core/src/formatter.rs` | 854 | machines | [HCO-01](#hco-01) | copy formatter scope emitter, single call |
| `apps/hotaru/crates/hotaru-core/src/grapheme.rs` | 226 | none |  | sparse cluster map with cached heap charge |
| `apps/hotaru/crates/hotaru-core/src/history.rs` | 1080 | machines | [HCO-02](#hco-02), [HCO-03](#hco-03) | block lifecycle, compression cursor, epoch and origin |
| `apps/hotaru/crates/hotaru-core/src/hyperlink.rs` | 342 | none |  | row-local link run table data structure |
| `apps/hotaru/crates/hotaru-core/src/input.rs` | 895 | machines | [HCO-04](#hco-04), [HCO-05](#hco-05) | kitty keyboard stack, mouse motion dedupe; key encoding pure |
| `apps/hotaru/crates/hotaru-core/src/kitty_clipboard.rs` | 1510 | machines | [HCO-06](#hco-06), [HCO-07](#hco-07) | OSC 5522 write transaction and grants |
| `apps/hotaru/crates/hotaru-core/src/lib.rs` | 311 | machines | [HCO-09](#hco-09), [HCO-20](#hco-20), [HCO-03](#hco-03), [HCO-08](#hco-08) | feed loop: parser fast path, reply budget, generation bump |
| `apps/hotaru/crates/hotaru-core/src/memory.rs` | 122 | machines | [HCO-08](#hco-08) | budget signal dirty/peak/pause |
| `apps/hotaru/crates/hotaru-core/src/parser.rs` | 417 | machines | [HCO-09](#hco-09) | explicit State enum plus UTF-8 and OSC sub-state |
| `apps/hotaru/crates/hotaru-core/src/position.rs` | 41 | machines | [HCO-03](#hco-03) | content epoch identity type |
| `apps/hotaru/crates/hotaru-core/src/row.rs` | 789 | machines | [HCO-12](#hco-12), [HCO-15](#hco-15), [HCO-08](#hco-08) | row wrap links, prompt marks, memory charge |
| `apps/hotaru/crates/hotaru-core/src/row/ascii.rs` | 190 | none |  | unsafe whole-cell store helper |
| `apps/hotaru/crates/hotaru-core/src/row/history.rs` | 535 | none |  | bounded row archive codec |
| `apps/hotaru/crates/hotaru-core/src/scan.rs` | 125 | none |  | pure SIMD byte classification |
| `apps/hotaru/crates/hotaru-core/src/screen.rs` | 3232 | machines | [HCO-10](#hco-10), [HCO-11](#hco-11), [HCO-12](#hco-12), [HCO-13](#hco-13), [HCO-14](#hco-14), [HCO-15](#hco-15), [HCO-16](#hco-16), [HCO-17](#hco-17), [HCO-18](#hco-18), [HCO-19](#hco-19), [HCO-20](#hco-20), [HCO-03](#hco-03), [HCO-04](#hco-04), [HCO-06](#hco-06), [HCO-07](#hco-07), [HCO-08](#hco-08) | terminal state and modes |
| `apps/hotaru/crates/hotaru-core/src/search.rs` | 451 | machines | [HCO-03](#hco-03) | match staleness validation; KMP matcher is pure |
| `apps/hotaru/crates/hotaru-core/src/style.rs` | 150 | none |  | style data types and SGR encoding |
| `apps/hotaru/crates/hotaru-core/src/unicode.rs` | 194 | explicit |  | BreakState table-driven grapheme breaks |
| `apps/hotaru/crates/hotaru-core/src/unicode_tables.rs` | 1530 | none |  | generated data tables |
| `apps/hotaru/crates/hotaru-io/src/clipboard.rs` | 316 | machines | [HCO-21](#hco-21) | pipe transfer state, partially explicit |
| `apps/hotaru/crates/hotaru-io/src/fd.rs` | 89 | machines | [HCO-22](#hco-22) | SIGPIPE block guard; nonblocking helper pure |
| `apps/hotaru/crates/hotaru-io/src/launch.rs` | 351 | machines | [HCO-23](#hco-23), [HCO-26](#hco-26) | shell bootstrap handoff; OSC 7 decoding pure |
| `apps/hotaru/crates/hotaru-io/src/lib.rs` | 19 | none |  | module declarations and re-exports |
| `apps/hotaru/crates/hotaru-io/src/pty.rs` | 373 | machines | [HCO-24](#hco-24) | PTY child and reaper lifecycle |
| `apps/hotaru/crates/hotaru-replay/src/bin/kitty-clipboard-replay.rs` | 267 | machines | [HCO-25](#hco-25) | test harness embedder model |
| `apps/hotaru/crates/hotaru-replay/src/main.rs` | 179 | none |  | CLI replay, linear feed loop |
| `apps/hotaru/crates/hotaru-sprites/src/braille.rs` | 114 | none |  | pure raster layout |
| `apps/hotaru/crates/hotaru-sprites/src/curves.rs` | 72 | none |  | pure raster |
| `apps/hotaru/crates/hotaru-sprites/src/geometric.rs` | 30 | none |  | pure raster |
| `apps/hotaru/crates/hotaru-sprites/src/lib.rs` | 256 | none |  | pure raster dispatch |
| `apps/hotaru/crates/hotaru-sprites/src/lines.rs` | 166 | none |  | pure raster |
| `apps/hotaru/crates/hotaru-sprites/src/lines_map.rs` | 117 | none |  | data table |
| `apps/hotaru/crates/hotaru-sprites/src/math.rs` | 204 | none |  | pure math |
| `apps/hotaru/crates/hotaru-sprites/src/powerline.rs` | 116 | none |  | pure raster |
| `apps/hotaru/crates/hotaru-sprites/src/vector.rs` | 718 | none |  | pure path flattening and fill |
| `apps/hotaru/resources/shell-integration/bash/hotaru.bash` | 35 | machines | [HCO-26](#hco-26) | load guard and OSC 7 dedupe |
| `apps/hotaru/resources/shell-integration/bash/rc.bash` | 10 | machines | [HCO-23](#hco-23) | bash bootstrap step |
| `apps/hotaru/resources/shell-integration/fish/hotaru.fish` | 18 | machines | [HCO-26](#hco-26), [HCO-23](#hco-23) | load guard and OSC 7 dedupe |
| `apps/hotaru/resources/shell-integration/zsh/hotaru.zsh` | 35 | machines | [HCO-26](#hco-26), [HCO-23](#hco-23) | load guard, re-hook, OSC 7 dedupe |
| `apps/hotaru/resources/shell-integration/zsh/.zshenv` | 16 | machines | [HCO-23](#hco-23) | zsh ZDOTDIR restore step |
