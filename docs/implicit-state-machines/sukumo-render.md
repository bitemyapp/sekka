# SKR: Sukumo engine: animation, text and raster

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/sukumo`. Audited at `8e0acbe5`; line numbers refer to that commit.

30 files, 0 pending. 17 implicit machines: 2 High, 6 Medium, 9 Low.

## Machines

<a id="skr-01"></a>

### SKR-01 — Render-thread layer tree linkage (parent links and forward references)
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/tree.rs:116` (`RLayer.parent`, `.sublayers` :115, `.mask` :117); `Compositor.waiting` :186, `Compositor.layers` :182
- **Encoding:** per-layer `parent: Option<LayerId>` kept in step with every other layer's `sublayers`/`mask` lists, plus `waiting: HashMap<child, parent>` for sublayers named before they arrived; no single owner of the edge set
- **States:** per layer id `Absent` → `Awaited(parent)` (in `waiting`) → `Present{parent: Some | None}` → `Gone`; representable invalid: `waiting[C] = P` with `P` gone, `C.parent = Some(P)` with `P` absent, `C` listed by two parents (parent = last linker)
- **Transitions:** parent's update names an absent sub: Absent→Awaited (`tree.rs:291`); fresh arrival adopts the waiting parent (`tree.rs:270-272`); named by a present parent: parent=Some (`tree.rs:289`); dropped from a parent's lists: parent=None (`tree.rs:281`), waiting entry removed (`tree.rs:284`); `gone`: layer removed (`tree.rs:214`), its children orphaned (`tree.rs:222`), only `waiting[gone]` removed (`tree.rs:227`)
- **Hazards:** edges maintained incrementally at five sites in `apply`; the gone path does not drop `waiting` entries whose value is the gone layer, so a child named by a deleted parent that arrives later adopts a dangling parent and never-arriving children leak entries; `touched()` (`tree.rs:306-328`) and the time chain in `local()` (`tree.rs:439-451`) walk `parent`, so a wrong link means a host is not marked dirty (missed redraw) or a layer's time ignores an ancestor's speed/offset
- **Reify:** Medium — five mutation sites keep a bidirectional relation consistent by hand, with dangling states representable but rarely reached

<a id="skr-02"></a>

### SKR-02 — Transition snapshot lifecycle
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/tree.rs:122` (`RLayer.transitions: Vec<(u64, Box<Node>)>`); also `RLayer.anims` :118, `AnimSpec.id` `ca/spec.rs:315`
- **Encoding:** snapshot list keyed by animation id, paired implicitly with the layer's current `anims`; whether one shows comes from `transition_progress` (`ca/spec.rs:352-357`) at the layer's time
- **States:** `NoSnapshot` → `Captured` → `Showing` (progress Some) → `Lapsed` (anim still on the layer but phase None: ended with fill removed, or not yet begun) → `Dropped`; representable invalid: transition anim present with no snapshot (layer was new in the commit), snapshot kept for a lapsed anim indefinitely
- **Transitions:** `apply` captures a node for transition ids not in the old anims, only for layers that already existed (`tree.rs:202-210`), pushes after the updates (`tree.rs:298-302`); `node_at` shows the latest with progress (`tree.rs:489-493`); `walk` bumps the generation every frame while showing (`tree.rs:687-689`); snapshots whose id left `anims` are dropped (`tree.rs:244`)
- **Hazards:** a `CATransition` added in the commit that creates its layer never gets a snapshot and silently draws without the transition; with `removedOnCompletion = NO` the snapshot (and the canvas `ImageData` Arcs inside it, `tree.rs:96-104`) lives as long as the animation stays, and while it holds the canvas pixels `paint_content`'s `Arc::get_mut` fails (`tree.rs:335-336`) so every canvas paint copies the whole canvas
- **Reify:** Medium — snapshot validity is tied to the anim list by id at two sites, and lapsed or missing states are representable without anyone noticing

<a id="skr-03"></a>

### SKR-03 — Retained paints and per-host frame scheduling (dirty / moving / last snapshot)
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/tree.rs:137` (`HostState.last` :138, `.dirty` :140, `.moving` :143); `WindowState.kept` :148, `.hosts` :149, `.composites` :152
- **Encoding:** kept paints (`Kept{target, rects, ops}`) whose rects shrink as later paints cover them; `composites` and the set of live hosts are derived from them by `reindex` (`tree.rs:158-169`); each host's `dirty`, `moving` and `last` snapshot decide whether it needs a frame and what it damages
- **States:** per (target, host) `Unindexed` → `Clean(last)` → `Dirty` (commit/paint touched it) → `Animating` → `Settling` (`moving`: one more frame after animation stops) → `Clean`; dropped when no kept paint composites it; representable: `dirty && moving`, a host in `hosts` whose kept paint was trimmed to nothing
- **Transitions:** `paint` trims older kept paints (`tree.rs:362-365`), keeps the new one (`tree.rs:373`), reindexes (`tree.rs:368`, `:374`), marks its hosts dirty (`tree.rs:376`) then records the drawn snapshot and clears dirty (`tree.rs:386-387`); `touched` sets dirty on hosts above changed layers (`tree.rs:324`); `frame` takes `last`, clears dirty, sets `moving = animating` (`tree.rs:579-581`), stores the new snapshot (`tree.rs:600`); `drop_target` / `drop_rects` reindex (`tree.rs:397`, `:411`); `drop_window` (`tree.rs:417`); `wants_frame` reads all three (`tree.rs:553`)
- **Hazards:** `dirty` can only be set on hosts that already have a `HostState`; `last` is advanced before the damage is known to be drawable; damage no kept paint covers is handed back as uncovered (`tree.rs:633`) and depends on the main thread repainting it
- **Reify:** High — three flags plus a snapshot and a derived index are updated from six entry points, and the suspected bug below loses redraws
- **Suspected bug:** `frame` clips damage to "where the host may draw" by target, not by host (`tree.rs:598` filters `damage` on `*t == target`), so every rect earlier hosts of the same target pushed is intersected with this host's clip too; with disjoint clips those rects become empty and are dropped (`tree.rs:603`) while their host's `last` was already advanced (`tree.rs:600`). Two layer-backed views animating at once in one window surface (two spinners): the host the stable `HashMap` order visits first never gets its animation redrawn while the other animates (confidence: high)
- **Related:** render loop calling `frame`/`wants_frame` in `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1745`, `:1782`, `:2403`
- **Reproduction:** Confirmed (unit): [test and output](reproductions.md#skr-03)

<a id="skr-04"></a>

### SKR-04 — Layer contents, canvas generations and their image-cache keys
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/tree.rs:119` (`RLayer.content`, enum `Content` :107-111, `RLayer.content_gen` :124); `Canvas.key` :91, `Canvas.generation` :92, `Canvas.px` :90; cache entries in `raster/images.rs:57-68`
- **Encoding:** partially explicit: `Content` enum, plus two change counters (`content_gen` for damage snapshots, `Canvas.generation` for the image cache) and a cache key minted per canvas (`tree.rs:780`), whose entries live in each drawing thread's image cache
- **States:** `None` | `Image` | `Canvas{key, generation}`; canvas `Fresh(gen 0)` → `Painted(gen n)`; its cache entry `Uncached` → `Cached(key, gen)` → `Forgotten` | `Evicted`; representable: cache entry for a key no layer holds
- **Transitions:** `ContentUpdate::None`/`Image` replace content, `content_gen += 1` (`tree.rs:250-255`); a canvas of another size replaces the old one with a new key, copying the overlap (`tree.rs:257-266`, `new_canvas` `:760-781`); `paint_content` forgets the key (`tree.rs:334`), copies on write (`tree.rs:335-336`), bumps both counters (`tree.rs:343-344`), touches hosts (`tree.rs:345`); a gone layer forgets its canvas key (`tree.rs:216`)
- **Hazards:** two counters bumped together but read by different consumers; `forget` reaches only the render thread's cache (`raster/images.rs:121-130`), not raster workers' copies of small canvases; any other holder of `px` (a worker cache, a transition snapshot) makes each paint copy the canvas
- **Reify:** High — the content/key/cache lifecycle spans two modules and three counters, with the leak below
- **Suspected bug:** when a canvas is replaced (resize, `tree.rs:258-265`) or swapped for `None`/`Image` (`tree.rs:250`, `:254`), the old `Canvas` is dropped without `images::forget(&[old.key])`; the render thread's cache keeps `Base::Shared(old px)` until LRU eviction against the 256 MiB budget (`raster/images.rs:105`, `:229-238`). Live-resizing a window with a layer-backed view makes a new canvas per size step, so the render thread retains up to the whole budget in dead canvas pixels (confidence: medium)
- **Related:** `desktop/aizome/sukumo/crates/sidestep-engine/src/backend/mod.rs:1062` (paint_content), `:1110` (apply), `:1172` (`ForgetImages`, sent only for main-thread image keys by `sidestep-ui/src/image.rs:30`)
- **Reproduction:** Confirmed (unit): [test and output](reproductions.md#skr-04)

<a id="skr-05"></a>

### SKR-05 — Animated GIF/WebP frame decoder cursor
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/codec.rs:242` (`Animation.frames: Option<Frames>`, `.next` :244)
- **Encoding:** an optional decoder iterator plus the index of the frame it yields next
- **States:** `Unstarted` (frames None) → `Positioned(next)` → `Exhausted` (iterator done, `next` keeps growing); representable: `Positioned` after a decode error
- **Transitions:** `frame(i)` with no iterator or `i < next` rebuilds the decoder and sets `next = 0` (`codec.rs:264-272`); each step advances `next` (`codec.rs:275-276`) and returns at `next > i` (`codec.rs:278-280`)
- **Hazards:** a frame that fails to decode (`codec.rs:277`) still advances `next`, and a later request for a higher frame continues from the broken iterator, compositing without the failed frame; a decoder that fails to construct leaves the old iterator and `next` in place
- **Reify:** Low — one struct, one method, restart rule is explicit enough

<a id="skr-06"></a>

### SKR-06 — Accent-keyed system palette cache
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/palette.rs:312` (`PALETTE: RwLock<Option<Palette>>`, `Palette.accent` :308)
- **Encoding:** global optional table set built for one accent
- **States:** `Unbuilt` → `Built(accent)`; no distinction between "default assumed" and "desktop said default"
- **Transitions:** `get` builds lazily with `DEFAULT_ACCENT` (`palette.rs:344-346`); `set_accent` rebuilds unless the accent is unchanged (`palette.rs:399-402`)
- **Hazards:** `set_accent(None)`'s "changed" result depends on whether anything called `get` first (true on an unbuilt palette, false after a lazy default build); harmless extra repaint at most
- **Reify:** Low — single field, two writers

<a id="skr-07"></a>

### SKR-07 — CoreGraphics path builder current point and derived caches
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/path.rs:45` (`Shape.start`, `.path` :43, `.made_rect` :50, `.drawn` :52)
- **Encoding:** the last element of `path` plus `start` give the CoreGraphics current-point state; `drawn` (OnceLock) and `made_rect` are caches cleared by `changed()` (`path.rs:86-89`)
- **States:** `Empty` (no current point) → `Open(subpath start)` → `Closed` (current point back at `start`); caches `Valid` / `Cleared`
- **Transitions:** `move_to` replaces a trailing move, sets `start` (`path.rs:100-108`); `segment` refuses on an empty path (`path.rs:111-118`); `close` adds at most one close (`path.rs:132-137`); `add_sweep` starts with a move or a line by emptiness (`path.rs:216-220`); `transformed` drops `made_rect` (`path.rs:312-316`)
- **Hazards:** `made_rect` and `start` are public; a caller must set `made_rect` after its last mutation, since every mutation clears it
- **Reify:** Low — rules are local and documented; the state is the element list itself

<a id="skr-08"></a>

### SKR-08 — Raster worker pool and scoped band latch
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/bands.rs:29` (`Pool.jobs`, `.wake`, `.workers` :32); `POOL` :39; `Latch.left` :75, `.done` :76, `.panicked` :77; `WaitFor` :99
- **Encoding:** lazily started workers that loop forever on a shared job stack; per `scoped` call a countdown latch, a panic flag and a drop guard that waits for borrowed jobs
- **States:** pool `Unstarted` → `Started(n)` (never stops); per call `Queued(k)` → `Running` (workers and caller) → `Drained` → `Waited` → `Done` | `Panicked`
- **Transitions:** `pool()` spawns workers, stopping at the first spawn failure (`bands.rs:49-68`); `scoped` creates the latch and guard (`bands.rs:197-199`), queues jobs (`bands.rs:201-216`), wakes workers (`bands.rs:217`), runs `mine` (`bands.rs:218`), drains what is left (`bands.rs:219-223`), waits via the guard (`bands.rs:224`, `:101-105`), asserts on panic (`bands.rs:225`); each job counts down (`bands.rs:204-209`, `:81-87`)
- **Hazards:** `Pool.workers` keeps the intended count (`bands.rs:48`) even if spawns failed, so callers band for threads that don't exist (the caller drains them); if `mine()` panics, unwinding skips the drain and `WaitFor::drop` waits for queued jobs, which with no live worker is a hang instead of a panic; the `'a` → `'static` transmute (`bands.rs:213`) is sound only through the guard; workers never exit
- **Reify:** Medium — thread and borrow lifetimes depend on an implicit invariant spread over the guard, the latch and the drain loop

<a id="skr-09"></a>

### SKR-09 — Per-thread image texture cache (budget, LRU, generations, halvings, tints)
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/images.rs:96` (`Cache.entries`, `.bytes` :98, `.budget` :99, `.clock` :100); `Entry.generation` :58, `.opaque` :61, `.halvings` :63, `.tints` :65, `.bytes` :66, `.used` :67; thread-local `CACHE` :110; reaper `REAPER` :164
- **Encoding:** per-thread map of entries with LRU stamps and byte accounting; staleness by `generation`; per-entry memo of opacity; tints kept FIFO (4)
- **States:** per key `Absent` → `Cached(gen, levels, tints)` → `Stale` (generation differs) → `Evicted` | `Forgotten`; invariant `cache.bytes == Σ entry.bytes`
- **Transitions:** `entry` drops a stale generation (`images.rs:193-196`), inserts (`images.rs:207-218`) and evicts (`images.rs:219`, `:229-238`); `draw` adds halvings (`images.rs:371-375`) and tints (`images.rs:385-390`), then updates accounting (`images.rs:395`, `:431`, `:478`) and evicts (`images.rs:432`, `:479`); `forget` (`images.rs:121-130`); `release` hands big entries to the reaper (`images.rs:139-156`, `:163-175`)
- **Hazards:** accounting is updated in seven places, and the early return at `images.rs:384` after halvings were pushed leaves them uncounted; each drawing thread (render, band workers, drawing threads) has its own 256 MiB budget, and `forget`/`release` reach only the calling thread's, so band workers' copies of small images (`bands.rs:110-126`) outlive their representation until LRU; a key drawn alternately at two generations thrashes (`images.rs:193`); a nested draw uses a throwaway cache (`images.rs:354-359`) that decodes encoded files every time
- **Reify:** Medium — invariants across fields are hand-kept at many sites, with per-thread scope that callers can't see
- **Related:** `ForgetImages` from `desktop/aizome/sukumo/crates/sidestep-ui/src/image.rs:30` via `backend/mod.rs:1172`

<a id="skr-10"></a>

### SKR-10 — Op-stream group nesting while rasterizing
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/ops.rs:108` (`layers: Vec<Layer>` in `run`); also `contents()` depth counter `raster/ops.rs:352`, `paint_timed` span scan `raster.rs:176-187`
- **Encoding:** a stack of group layers pushed by `BeginGroup` and popped by `EndGroup`; two other parsers re-derive the same nesting with depth counters
- **States:** `TopLevel` → `InGroup(depth)` → back; representable: unmatched `EndGroup`, unclosed `BeginGroup`
- **Transitions:** `BeginGroup` sizes a layer from what its ops reach and pushes it (`ops.rs:111-145`); ops draw into the top layer (`ops.rs:151-157`); `EndGroup` pops and composites (`ops.rs:146-150`, `close_group` `:171-213`); unclosed groups close at the end (`ops.rs:163-165`)
- **Hazards:** three parsers must agree on nesting; for an unbalanced stream `paint_timed` (`SIDESTEP_TRACE_FRAMES=tiles`) runs a lone `BeginGroup` as a one-op span and then draws its contents outside any group, unlike `paint`; the one-slot clip-mask memo `Masks.last` (`ops.rs:43`, `:54-67`) stays valid only by holding the clip's Arc
- **Reify:** Low — per-call stack, well contained

<a id="skr-11"></a>

### SKR-11 — Font collection open/prewarm and the squashed-name cache
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/text/fonts.rs:474` (`SHARED: OnceLock<Shared>`); `Shared.fcx` :461, `Shared.squashed` :468; prewarm `ONCE` :482
- **Encoding:** once-opened shared font context (each thread's `Ctx.fcx` is a clone, `text.rs:70`, synced through fontique's shared store), plus a name map cache that `install` invalidates
- **States:** collection `Unopened` → `Opening` (prewarm thread, or first caller blocking in the OnceLock) → `Open(fontconfig)` | `Open(scanned dirs)`; squashed map `Unbuilt` → `Built` → `Invalidated`
- **Transitions:** `prewarm` spawns the opener once (`fonts.rs:481-488`); `Shared::open` falls back to scanning directories when fontconfig panics (`fonts.rs:504-526`) and applies `SIDESTEP_FONT` overrides (`fonts.rs:527-528`); `squashed()` builds and stores (`fonts.rs:571-577`); `install` registers and clears it (`fonts.rs:1085`, `:1087`); the per-thread face cache starts over at 512 (`fonts.rs:744-746`)
- **Hazards:** `squashed()` checks, builds from this thread's clone, then writes without re-checking (`fonts.rs:571-576`): an `install` clearing the map in between leaves a stale map without the new families cached until the next install, so `spec_named("NewFamily-Bold")` misses; registrations reach other threads only because the collection is created `shared: true` (`fonts.rs:503`)
- **Reify:** Low — once-init plus a single invalidated cache; a generation counter would close the race

<a id="skr-12"></a>

### SKR-12 — Font-file face registry (private families, by blob, by content, by name)
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/text/fonts.rs:109` (`DataRegistry.by_blob` :113, `.by_content` :114, `.named` :117); `DATA` :120; `DataFace.in_collection` :73
- **Encoding:** a bounded blob memo, a permanent content index and a name table, plus a per-face once-flag recording whether a collection took it
- **States:** per file `Unknown` → `Peeked` (face made, not remembered) → `Registered` → `Named` ⇄ `Unnamed`; per face `in_collection` `Unasked` → `Collected(true | false)`
- **Transitions:** `register_data` finds, makes outside the lock, re-finds and inserts (`fonts.rs:172-186`); `remember_blob` clears the blob map at 1024 (`fonts.rs:150-155`); `register_named` / `unregister_named` (`fonts.rs:262-275`); `collected` registers in the asking thread's context once (`fonts.rs:81-87`, used at `:759`)
- **Hazards:** `peek_data` (`fonts.rs:190-192`) makes faces that a later `register_data` doesn't find, so the same file can end up under two private families; faces are never let go by design
- **Reify:** Low — append-mostly registry with a double-checked insert

<a id="skr-13"></a>

### SKR-13 — Terminal grid `FontSet` (primary face, bounded faces, resolution and layout caches)
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/text/grid.rs:187` (`FontSet.faces` :190, `.primary` :191, `.resolutions` :193, `.resolution_bytes` :194, `.cache` :195, `.cache_bytes` :196, `.buffer` :197)
- **Encoding:** a worker-owned shaper: face list capped at 128 with no eviction, two caches that start over at their bounds, a lent `UnicodeBuffer`, a caller-owned cancel flag
- **States:** `Built(primary)` → faces `0..128` → `Full`; caches `Filling` → `Reset`; buffer `Held` / `Lent`
- **Transitions:** `new` resolves and adds the primary (`grid.rs:232-233`); `add_face` registers globally, then refuses past the cap (`grid.rs:337-343`) or appends (`grid.rs:401`); `resolve` clears and inserts (`grid.rs:441-446`); `shape` checks cancel (`grid.rs:458`, `:482`), hits (`grid.rs:475`), clears and inserts (`grid.rs:508-515`); `shape_run` takes and returns the buffer (`grid.rs:525`, `:630`)
- **Hazards:** once 128 faces are loaded, any text needing a new fallback face makes `shape` fail with `LimitExceeded` for good (failed resolutions aren't cached, so each call re-runs the fallback query), and Hotaru reports the whole job as `FontFailed`; `fonts::register` runs before the cap check, so refused faces still enter the process-wide registry
- **Reify:** Medium — several bounded caches with one unbounded-then-failing component; the cap's failure mode is user-visible
- **Related:** `desktop/aizome/apps/hotaru/src/font.rs:101-133` (worker owning the `FontSet`, epoch and cancel)

<a id="skr-14"></a>

### SKR-14 — Two-generation text caches (layouts, glyph strikes, shared glyph images)
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/text/layout.rs:748` (`Cache.new` :749, `.old` :750, `.len` :751, `.bytes` :752, in `Ctx.layouts` `text.rs:57`); also `text/raster.rs:38` (`Glyphs.new` :42, `.old` :43, `.bytes` :45, `.faces` :40) and `text/raster.rs:138` (`Shared`, static :144)
- **Encoding:** structurally identical caches: a newer and an older map, a byte/entry count for the newer only, promotion on hit, rotation when the newer one fills
- **States:** entry `Absent` → `New` → `Old` (after a rotation) → `New` (hit) | `Dropped` (second rotation)
- **Transitions:** layouts: promotion (`layout.rs:770-776`), insert (`layout.rs:784-789`), rotate (`layout.rs:792-795`); glyphs: rotate per run (`text/raster.rs:179-182`), promote from old (`text/raster.rs:215`), count (`text/raster.rs:234`); shared: promote (`text/raster.rs:152-155`), rotate on put (`text/raster.rs:160-163`)
- **Hazards:** only the newer generation is counted (peak about twice the bound); every drawing thread has its own glyph cache (render thread, band workers `raster/bands.rs:35`, `ca/render.rs:650`, `raster/pixels.rs:98`) of up to 24 MiB beside the shared one; shared promotion adds bytes without rotating; layout keys hash `Arc<Face>` pointers (`layout.rs:468`), so the per-thread face start-over (`fonts.rs:744`) strands old entries until rotation; the lent parley layout `Ctx.scratch` (`layout.rs:1418`) comes back only on the non-truncating path (`layout.rs:1182`) and in `glyphs.rs:365`
- **Reify:** Low — rotation and promotion are centralised in each cache's two methods

<a id="skr-15"></a>

### SKR-15 — Incremental TextKit frame layout after edits
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/text/lines.rs:804` (`Frame.paragraphs` :805, `.len` :808, `.bytes` :809, `.reach` :811); `FrameParagraph.start/byte/top` :816-825, `ParagraphLines.complete` :321, `Line.context` :167
- **Encoding:** a cached layout of a text whose validity depends on every edit being reported; paragraph offsets are shifted, not recomputed; `complete` and `context` decide which old lines can be kept
- **States:** `Fresh` → `Edited` (in place, or by splicing re-laid paragraphs) → `Rebuilt` (fallback); per paragraph `Complete` | `Partial` (line limit)
- **Transitions:** `Frame::new` (`lines.rs:885-897`); `edit` falls back to a rebuild for limits or bad ranges (`lines.rs:1123-1126`), widens back over a lone CR (`lines.rs:1136-1138`), edits in place (`lines.rs:1142`, `:1185-1250`) or splices (`lines.rs:1156-1158`), advances `len` by the delta (`lines.rs:1161`), takes `bytes` from the text (`lines.rs:1162`), updates `reach` (`lines.rs:1164-1165`), shifts later paragraphs (`lines.rs:1173-1177`); `relay_lines` rejoins old lines where position and context match (`lines.rs:1304-1313`)
- **Hazards:** `edit` trusts the caller's `old`/`inserted`; only `old.end > len` and inverted ranges are rejected, so a missed or coalesced edit makes `len` drift from the text with no self-check, and later edits splice the wrong paragraphs; frames with `max_lines > 0` always rebuild
- **Reify:** Medium — transitions are centralised in `edit`, but correctness rests on an external edit stream the frame can't verify

<a id="skr-16"></a>

### SKR-16 — Text layout worker pool batches
- **Where:** `desktop/aizome/sukumo/crates/sidestep-engine/src/text/pool.rs:23` (`Batch.slots` :25, `.next` :27, `.done` :28, `.all_done` :29); `enum Slot` :33-38; `WORKERS` :65
- **Encoding:** partially explicit: `Slot` is `Waiting` / `Taken` / `Done`, but claiming goes through an atomic cursor and completion through a separate counter and condvar
- **States:** batch `Sent` → `Working` → `AllDone` → `Collected`; slot `Waiting` → `Taken` → `Done(result | panicked)` → `Taken` (collected)
- **Transitions:** `map` builds and sends (`pool.rs:89-99`), works (`pool.rs:100`), waits (`pool.rs:101-104`), collects (`pool.rs:106-113`); `work` claims an index (`pool.rs:49`), takes (`pool.rs:51`), stores the result (`pool.rs:54`), counts and notifies (`pool.rs:55-59`)
- **Hazards:** `work` on the caller's thread calls `with_ctx` (`pool.rs:53`), whose `RefCell` isn't reentrant (`text.rs:68`): a `map` from inside `with_ctx` would turn every caller-run job into a caught panic and an empty layout or frame (`lines.rs:917-920`); current callers (`layout.rs:659`, `lines.rs:914`) stay outside it; a panicking job leaves that thread's `Ctx` caches as parley left them
- **Reify:** Low — small, panic-safe, and the slot enum already carries most of the state

<a id="skr-17"></a>

### SKR-17 — (test harness) Headless sway runner lifecycle
- **Where:** `desktop/aizome/sukumo/scripts/headless-wayland:39` (`sway_pid`, `app_pid` :49, `XDG_RUNTIME_DIR` :16)
- **Encoding:** shell variables and a temporary runtime directory; readiness is the socket file appearing
- **States:** `Setup` → `SwayStarting` → `SwayReady` → `AppRunning` → `Then/Shot` → `AppStopped` → `SwayStopped` → exit
- **Transitions:** sway started (`headless-wayland:39-40`); socket poll, 100 × 50 ms then `exit 1` (`headless-wayland:41-46`); app started (`headless-wayland:48-49`); THEN then optional shot and kill (`headless-wayland:50-54`) or shot after a delay and kill (`headless-wayland:55-58`); wait for the app (`headless-wayland:61`); kill sway (`headless-wayland:62`)
- **Hazards:** no `trap`: the socket timeout exits with sway still running, and the temporary runtime directory is never removed; `wait` after `kill` has no timeout if the app ignores SIGTERM
- **Reify:** Low — test harness
- **Suspected bug:** under `set -eu` (`headless-wayland:15`) a failing `grim` (`headless-wayland:53`, `:57`; missing binary, compositor gone) aborts the script before `kill $app_pid` and `kill $sway_pid`, leaving the app and sway running in the container (confidence: medium)
- **Reproduction:** not attempted; only suspected bugs on High machines were reproduced

## Already explicit

- None in this slice.

## Audit ledger

Every in-scope file in this slice: `machines` (documented above), `explicit` (only already-explicit machines), `none` (no stateful lifecycle) or `pending` (not yet audited).

| File | Lines | Result | Machines | Note |
| --- | ---: | --- | --- | --- |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/keyframe.rs` | 236 | none |  | pure keyframe interpolation |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/math.rs` | 848 | none |  | pure transform, timing curve and spring math |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/mod.rs` | 13 | none |  | module declarations |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/props.rs` | 1020 | none |  | plain layer property data and key paths |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/render.rs` | 678 | none |  | pure op emission; thread-local glyph cache see SKR-14 |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/spec.rs` | 542 | none |  | pure CAMediaTiming math; phase derived from time; removal lives in appkit |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/ca/tree.rs` | 858 | machines | [SKR-01](#skr-01), [SKR-02](#skr-02), [SKR-03](#skr-03), [SKR-04](#skr-04) | render-thread layer trees, transitions, frame scheduling, canvases |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/codec.rs` | 500 | machines | [SKR-05](#skr-05) | animated frame decoder cursor; rest pure codecs |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/color.rs` | 26 | none |  | pure sRGB math |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/palette.rs` | 496 | machines | [SKR-06](#skr-06) | accent-keyed palette cache |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/path.rs` | 467 | machines | [SKR-07](#skr-07) | path builder current point and drawn cache |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/raster.rs` | 362 | machines | [SKR-10](#skr-10) | paint_timed re-parses group nesting |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/bands.rs` | 397 | machines | [SKR-08](#skr-08) | raster worker pool and scoped latch |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/effects.rs` | 317 | none |  | shadow blur and gradients; thread-local scratch reuse |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/images.rs` | 1022 | machines | [SKR-09](#skr-09) | per-thread texture cache; reaper thread lazy init |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/masks.rs` | 237 | none |  | immutable alpha masks |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/ops.rs` | 1484 | machines | [SKR-10](#skr-10) | group layer stack; one-slot clip mask memo |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/raster/pixels.rs` | 360 | none |  | pack/unpack bitmap formats; thread-local scratch |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/text.rs` | 78 | none |  | per-thread Ctx lazy init; reentrancy noted in SKR-16 |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/text/fonts.rs` | 1141 | machines | [SKR-11](#skr-11), [SKR-12](#skr-12) | collection open, squashed cache, file face registry |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/text/glyphs.rs` | 462 | none |  | pure CTLine layout; loop-local accumulators |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/text/grid.rs` | 972 | machines | [SKR-13](#skr-13) | terminal FontSet caches and face cap |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/text/layout.rs` | 2399 | machines | [SKR-14](#skr-14) | two-generation layout cache; compute is pure |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/text/lines.rs` | 1441 | machines | [SKR-15](#skr-15) | incremental frame relayout after edits |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/text/lines/tests.rs` | 841 | none |  | tests |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/text/outline.rs` | 77 | none |  | pure glyph outline extraction |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/text/pool.rs` | 124 | machines | [SKR-16](#skr-16) | text worker pool batches |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/text/raster.rs` | 587 | machines | [SKR-14](#skr-14) | two-generation glyph caches |
| `desktop/aizome/sukumo/crates/sidestep-engine/src/text/tests.rs` | 750 | none |  | tests |
| `desktop/aizome/sukumo/scripts/headless-wayland` | 63 | machines | [SKR-17](#skr-17) | test harness: headless sway runner |
