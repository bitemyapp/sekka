# SKR: Sukumo engine: animation, text and raster

Part of the [implicit state machine catalog](../implicit-state-machines.md), which defines the fields below. Paths: `desktop/aizome/sukumo`. Audited at `8e0acbe5`; line numbers refer to that commit.

30 files audited. 17 implicit machines: 2 High, 6 Medium, 9 Low. This page has the entries of the High machines; the [index](../implicit-state-machines.md#index) records the rest.

## Machines

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
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#skr-03)

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
- **Reproduction:** Confirmed (unit): [summary](reproductions.md#skr-04)
