# Relax and Buck2 + NativeLink: cache and worktree comparison

On `wx-workstation`, Relax was faster with empty build caches and for the
tested edits in an already-warm worktree. Buck2 + NativeLink was faster for
unchanged reruns and for a different worktree containing identical or nearly
identical sources. Buck2 also shared substantially more disk space across
worktrees.

These new measurements run **the same component tests**, not equivalent
whole-desktop native builds. Relax JIT-compiles the paths exercised by the tests;
Buck2 produces native test executables with rustc. The historical full native
build measurements are reported separately below. No Relax full native-build
time or equivalent whole-build storage figure has been established.

## Measured component-test latency

Each entry is the sum of two sequential commands: `aizome-kit --lib` (40 tests)
and `aizome-soroban --lib` (11 tests). Source edits add one executed calculator
test, so those entries run 52 tests. All tests pass under both engines.

| Scenario | Relax | Buck2 + NativeLink | Cache state |
| --- | ---: | ---: | --- |
| Empty writable build caches, first worktree A | **22.45 s** | 63.50 s | Buck: 0/243 command hits, 214 rustc executions; Relax: no prior application cache |
| Unchanged rerun in A, median of 3 | 1.63 s | **0.71 s** | Existing worktree state; Buck runs no build commands |
| First invocation in identical worktree B | 16.52 s | **4.86 s** | Buck: 243/243 NativeLink command hits, no compiler executions |
| First invocation in worktree C, README-only edit | 15.73 s | **6.03 s** | Build inputs unchanged; Buck: 243/243 NativeLink command hits |
| First invocation in worktree D, new calculator source/test edit | 14.80 s | **7.79 s** | Buck: 242/243 NativeLink command hits (**99.6%**), one rustc execution |
| Calculator source/test edit in already-warm C | **1.95 s** | 2.78 s | Buck recompiles one test crate; Relax reuses startup code |
| Widget-kit public function added and called by a new calculator test in warm C | **2.80 s** | 7.85 s | Buck executes three rustc actions; Relax reuses most startup code |

The cold, relocated-worktree and edit entries are single observations, not
statistical estimates. The unchanged reruns are three-run medians. Additional
warm medians after worktree changes and edits were 1.65–1.74 s for Relax and
0.66–0.73 s for Buck2. [All observations and ranges](summary.json) are retained.
A separate three-run check with Relax's verbose diagnostics disabled measured
**1.58 s versus 0.71 s** for unchanged A; it did not change the conclusion.

The README edit isolates relocation with irrelevant repository churn. Worktree
D supplies the stronger “mostly identical” case: a real, previously unseen Rust
edit, made **before either engine had build state for that path**. Its new test
actually executes. C's later edits instead measure the edit loop within a
worktree already built by each engine.

### Interpreting cache percentages

There is no common cache-percentage denominator between these systems.
Buck2's percentages above count cacheable build commands looked up in NativeLink;
they exclude simple bookkeeping actions, local action-cache revalidations and
the separately executed tests. Unchanged warm invocations use the daemon's
existing graph and outputs, so “no build commands” is more accurate than a new
100% remote-cache hit rate.

Relax caches parsed crates, interfaces, function code and startup images. Its
lazy-code hit counter excludes code installed from a startup image. For example,
the first warm widget-kit run prelinked **10,834 startup slots**, then reported
only **8 lazy hits and 17 misses**. Calling that a 32% overall cache hit rate
would misrepresent an almost completely reused run. With a new worktree path B,
the same target prelinked zero startup slots and reported **8,440 lazy hits and
2,186 misses**. It reused considerable dependency code but still did more work.
The [summary](summary.json) preserves these counters separately rather than
manufacturing comparable percentages.

“Cold” means empty writable caches at the start of the two-command workload.
The second component can reuse work just done by the first. Toolchains and
dependency sources were already present, and the OS page cache was not flushed.
This measures low build-cache availability, not a cold machine or dependency
download.

## Physical disk consumption

These are **compressed file-data extent unions**, measured with one `compsize`
pass over each engine's complete selected build-cache/output roots on Btrfs
`compress=zstd:1`. Shared/reflinked extents are counted once. Units are decimal
MB; these are not sums of `du` output.

| Cumulative state | Relax | Buck2 + NativeLink |
| --- | ---: | ---: |
| After the first cold worktree | 626.16 MB | 721.32 MB |
| After identical worktree B | 898.46 MB | 722.46 MB |
| After README-only worktree C | 1,169.54 MB | 723.75 MB |
| After C's calculator edit | 1,238.71 MB | 734.77 MB |
| After C's shared widget-kit edit | 1,282.70 MB | 850.24 MB |
| After fresh source-edited worktree D | 1,554.11 MB | 862.26 MB |

Moving from one to three worktree paths added **543.38 MB for Relax versus
2.43 MB for Buck2**. The two successive increments were approximately
272.30/271.08 MB and 1.14/1.29 MB, respectively. Those intervals also include
three unchanged reruns of the preceding worktree; they are observed workload
increments, not isolated source-checkout costs or guarantees for every future
worktree. For D, an explicit snapshot immediately before the fresh edited
worktree gives **+271.05 MB versus +12.01 MB**.

Relax's selected root includes its compiler caches, parsed images, startup
images, native bridge outputs, both Cargo native target trees, and generated
build-script files. Buck's roots include NativeLink's CAS and action cache, the
local content store, and each worktree's new `buck-out` isolation. The latter
also contains its materializer state and local build logs. Root lists, ordinary
logical/allocated `du` readings, and physical receipts are retained in
[results.json](evidence/results.json) and the indexed `*-storage.log.gz` files.

This table describes **writable build state**, not total installed disk use.
It excludes existing Git checkouts/objects, downloaded registry sources, compiler
and Nix toolchain installations, NativeLink container images/runtime state,
benchmark evidence, and global filesystem metadata. Buck's separate daemon
control directories were 12 KiB allocated apiece for A/B/C at the recorded
check; they are outside this table. Relax requires no NativeLink service.
The historical full deployment accounting below includes a broader scope and
must not be compared directly with these component-cache numbers.

Why relocation costs Relax space is consistent with its implementation:
[crate fingerprints](https://github.com/bitemyapp/relax/blob/d09dcd17fa766b42e2f304fcdecfb489b2d00078/libs/relax/front/src/crate_image.rs#L488)
include file/root paths, and the
[native bridge key](https://github.com/bitemyapp/relax/blob/d09dcd17fa766b42e2f304fcdecfb489b2d00078/libs/relax/front/src/native.rs#L143)
includes package identity derived from the manifest directory. A shared
`MAKEPAD_RUN_CACHE_DIR` therefore permits reuse without making relocated
workspace crates fully path-independent. The measured outcome is narrower than
a claim that all Relax cache entries are duplicated: registry/std paths remain
stable and do reuse cached work. Buck2's local-CAS mode clones cache hits and
shares output extents with the CAS, which explains the much smaller observed
worktree increments.

## Historical full native-build evidence

The [November 6 final storage run](https://github.com/bitemyapp/sekka/blob/54d6089f2cdddac0bb443faab6eb544e8be22d92/docs/test-results/buck2/2026-11-06-final/README.md)
used the same Buck2 experiment source revision and workstation, with 67 native
Cargo roots in the dev build. It is an older run with its own recorded machine
state and cache configuration, not another repetition of today's component
test measurement.

| Full native-build scenario | Buck2 + NativeLink |
| --- | ---: |
| Cold dev build, 631 compiler executions | 83.57 s |
| Unchanged build | 0.21 s |
| Four fresh identical worktrees, all 706 commands cached | 3.87–4.42 s; no compiler executions |
| Leaf application edit, one compiler execution | 1.62 s |
| Shared theme edit, 54 compiler executions | 15.79 s |
| Fresh release worktree, all 742 commands cached | 3.72 s |

Its broader **active deployment physical union** was 8.46 GB after the cold
dev build and 12.86 GB after edits and a revert. Adding four identical worktrees
then cost **14.0–14.8 MB each**, reaching 12.92 GB. That union includes retained
toolchain/service inputs and other owned state, unlike the smaller cache-only
table above. Five concurrently divergent worktrees reached 23.23 GB; after the
full plan, including cache eviction and release/test work, the total was
20.16 GB. Cache size depends on retained variants and policy, not just the
number of source directories.

For frequent new or mostly identical worktrees, the current evidence favors
Buck2 + NativeLink for both native-output reuse and additional disk space.
For the measured JIT test loop, Relax reduces cold and small-edit latency but
currently pays more when the workspace path changes. There is no measured
Relax equivalent of the 67-root native build or its deployable outputs.

## Environment, validity and reproduction

- Host: `wx-workstation`, Linux x86-64/NixOS, 128 logical CPUs, approximately
  250 GiB RAM, Btrfs with zstd compression. Both engines use a budget of 16.
  Unrelated host services remained running; this is not a dedicated or
  CPU-affinity-controlled benchmark. No global cache flush or host cleanup ran.
- All four existing worktrees have tracked source revision
  `7fef66a8ae5b3f592194dd2104b1afa354ca6dfb`. Each had fresh writable state for
  this experiment. The worktrees themselves and their Git objects were reused,
  not newly cloned or charged as new disk allocation. All temporary source
  changes were restored, and tracked worktree status is clean.
- Relax revision `d09dcd17fa766b42e2f304fcdecfb489b2d00078`; runner SHA-256
  `ceacd35f0729c6051838774363f32dee99bb3fe6e142c1d53a9fb54b43c48fd9`.
  Its `--allow-c` bridge uses rustc 1.99.1 in the locked root Relax development
  shell. Buck's pinned native toolchain uses rustc 1.99.0 and LLVM 21.1.8;
  [toolchain identities](toolchains.json) and
  [Relax environment](evidence/results.json) record this difference.
- Buck2 is the experiment's 2026-10-15 engine with its local-content-store
  changes. The fresh NativeLink service has a 16 GiB CAS, 256 MiB action cache,
  and authenticated local-CAS writer. `--cache cache` means **local execution
  backed by NativeLink caching**, not execution on remote workers. The same
  Buck isolation name is used in every worktree because it enters action paths.
- Relax uses one shared `MAKEPAD_RUN_CACHE_DIR` across all worktrees,
  `--no-live --allow-c --offline`, and existing dependency inputs. Its resolver
  rejects `--locked`; an initial setup attempt discovered that before doing
  build work and is not a timing observation. The measured commands omit it.
  Manifest/lockfile identities and clean tracked status are recorded.
- Timings include command startup, test execution, normal cache persistence,
  and Buck's post-build sharing pass; development-shell entry and inventory
  scans are excluded. A blocking process wait avoids timeout-poll rounding.
  The harness waits for any orphaned Relax native Cargo descendants separately;
  no extra native tail was observed here. It does not wait for persistent Buck
  daemons as if they were build work.
- The main plan validates 96 package invocations, plus 12 invocations in the
  non-verbose warm check. Native event logs establish successful local test
  execution, exact build IDs, no retries, and compiler/action counts. Relax
  reports no code-link failures or dependency-certificate refusals. All 287
  exported evidence files pass the [index](evidence/index.json) hash check.
  Private service credentials are excluded and scanned for before export.
- The four owned Buck daemons and the new NativeLink service were stopped
  afterward ([cleanup](cleanup.log)); their caches were retained. Other
  worktrees and services were left alone. These are component/build-cache
  observations, not desktop, image, installation or hardware qualification.

The exact run-specific [driver](harness/compare.py),
[non-verbose check/export](harness/finalize.py), and
[fresh edited worktree follow-up](harness/fresh-edit.py) retain commands,
source mutations, timing and inventory methods. They contain this run's paths;
reproduction requires a new owned benchmark directory, a new private service
manifest/client, clean worktrees at the recorded revision, and a common unused
Buck isolation name. Do not rerun them over the retained caches and call that
cold. Regenerate and validate the report's [summary](summary.json) with:

```sh
python3 harness/analyze.py .
```
