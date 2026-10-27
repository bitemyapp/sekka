# Memory and heap profiling

Start with process memory, then use allocation traces to explain the
anonymous part. The profiling tools are optional:

```sh
nix develop .#aizome-profiling
scripts/aizome-memory
scripts/aizome-memory --json > memory.json
scripts/aizome-memory 1234 5678       # selected process IDs
```

`aizome-memory` reads Linux `/proc` without injecting code or restarting
anything. **PSS** apportions shared pages between processes; adding RSS
double-counts shared code and shared buffers. The anonymous, file-backed,
and shared-memory columns help distinguish heap/stacks from executable
code and Wayland buffers. Anonymous PSS includes allocator retention and
thread stacks, so it is not the same as live allocated bytes. GPU memory,
kernel allocations and unmapped file cache are outside these process
totals.

## Shared memory retained after resizing

Sidestep keeps a shared-memory pool for each process. Its SCTK allocator
grows the pool geometrically and reuses freed slots; resizing a window
drops its old buffer handles but does not shrink the backing pool. The
initial 4 MiB capacity is not automatically all resident, whereas
`reserve_buffers` deliberately faults pages in ahead of drawing. These
warm pages avoid allocation/page-fault delays on the next frame or panel
opening. A high SHM reading after a large window closes can therefore be
retained capacity rather than live heap or a leaked window.

The [Wayland protocol allows pool growth only](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_shm_pool).
Reclaiming unused pages needs work in Sidestep's renderer/allocator, where
buffer ownership and compositor release events are known. Aizome Kit's
window API cannot safely shrink this storage itself. A future policy
should retain a modest warm reserve, reclaim only genuinely unused storage
after sustained inactivity, and measure the next interaction's latency.
Replacing the pool also requires preserving or redrawing root, tile and
decoration buffers. This is separate from allocator tuning for Rust heap
allocations.

## Capture an app's allocations

Use the installed `aizome-profile`, or `scripts/aizome-profile` from the
checkout. It uses Heaptrack with Rust symbol demangling supplied by Nixpkgs.

```sh
scripts/aizome-profile heap aizome-soroban
# Use the app, then close it. The command prints the recording's path.
scripts/aizome-profile interpret aizome-profiles/heaptrack.EXAMPLE.raw.zst
scripts/aizome-profile view aizome-profiles/heaptrack.EXAMPLE.zst
scripts/aizome-profile summary aizome-profiles/heaptrack.EXAMPLE.zst
```

Use the actual filename printed by Heaptrack; its compression can be
`.gz` instead of `.zst`. `AIZOME_PROFILE_DIR` changes the destination.
The helper records raw addresses first and resolves debug information
afterward, so DWARF parsing is outside the capture. Keep the executable,
libraries and matching symbols available until interpretation finishes.

The GUI shows peak consumers, allocation counts, temporary allocations,
allocation sizes, timelines and flame graphs. High allocation counts or
temporary allocations reveal churn even when the retained heap is small.
Compare a repeatable idle period and a specific interaction (opening a
folder, resizing a window, switching a Desk), with the same data and output
scale. The standard Rust allocator is supported without source changes.
Heaptrack sees allocator calls; custom pools or direct `mmap` allocations
may need annotations or a different profiler.

`summary` disables Heaptrack's backtrace merging: the pinned version
crashed while merging a captured Rust shell trace. Individual stacks
retain the allocation data and source locations.

Profiling adds allocation and stack-unwinding overhead. Its RSS and timing
are not normal-session measurements. Confirm any improvement afterward
with an uninstrumented run and `aizome-memory`/the frame benchmark. Close
the profiled process normally to finish the recording. Launching under
Heaptrack includes startup allocations; attaching later misses those and
Heaptrack documents injection/detachment as potentially unstable.

## Separately installable symbols

The normal Nix package produces an additional `debug` output, exposed as
`aizome-debug`. It contains symbols from the **same build** as `aizome`,
including the compositor's core and logic libraries and the standard
apps. Installing the desktop does not retain this output automatically.

```sh
nix build .#aizome --out-link result
nix build .#aizome-debug --out-link result-debug
export AIZOME_DEBUG_DIRS="$(readlink -f result-debug)/lib/debug"
scripts/aizome-profile heap ./result/bin/aizome-soroban
scripts/aizome-profile interpret aizome-profiles/heaptrack.EXAMPLE.raw.zst
```

For a running installed release, obtain `aizome-debug` from that exact
release and locked inputs. A newly built checkout's symbols do not describe
an older installed executable. Nixpkgs stores the split ELF files under
`lib/debug/.build-id`; Heaptrack's interpreter finds them using
`--debug-paths`, which the helper supplies from `AIZOME_DEBUG_DIRS` (or
`NIX_DEBUG_INFO_DIRS`). Multiple directories are colon-separated. Set the
path before **interpretation**, not only before opening the GUI: the GUI
reads the already-resolved traces.

For GDB, use its Nixpkgs search path:

```sh
export NIX_DEBUG_INFO_DIRS="$AIZOME_DEBUG_DIRS"
gdb ./result/bin/aizome-soroban
```

The debug output also retains a `.source` link to the exact Aizome and
Sidestep source used for that build. GDB reports the compilation directory
as `/build/aizome-source/desktop/aizome`; substitute the source root to open the retained
source, even without a checkout:

```sh
symbol_file=$(readlink -f result-debug/lib/debug/aizome-soroban)
source_tree=$(readlink -f "${symbol_file%.debug}.source")
gdb ./result/bin/aizome-soroban \
  -ex "set substitute-path /build/aizome-source $source_tree" \
  -ex 'list aizome_soroban::main'
```

The workspace's release profiles emit `line-tables-only` DWARF, enough for
source locations and stack traces; split files also retain ELF symbols.
They do not provide full local-variable/type inspection. Nix's split-debug
hook normally injects `-g` into Rust outside Cargo; the package removes
that injected override so the profile and a matching `aizome-dev` build
retain the same settings. Cargo stripping is disabled until Nix extracts
the debug output. Nix still strips the installed runtime binaries.
The toolchain's prebuilt `libstd` has no build ID or DWARF to extract, so
this output cannot add source lines for that library.

DWARF debug sections are normally **not mapped into a running process**.
Splitting them keeps symbols out of the runtime closure and makes them an
optional download; it is not a live-heap reduction. The previous Nix package
already stripped its DWARF; the new output preserves those matching symbols
for optional use. The profiler/debugger uses memory when it reads them.
Unwind tables such as `.eh_frame` are
different: unwinding can need them at runtime, and stripping debug data
does not remove them. Do not disable unwinding for the compositor's
reloadable logic: its panic recovery uses it.

Tracy can correlate allocations with frames and instrumented code regions,
but its memory capture needs explicit allocation/free events, typically
through an opt-in Rust allocator wrapper. Separate symbols alone do not
enable heap capture. Aizome does not currently add that instrumentation or
its runtime dependency; Heaptrack provides allocation traces without
changing the application. See [Tracy's allocation hooks](https://github.com/wolfpld/tracy/blob/master/public/tracy/Tracy.hpp).

## The compositor and shell

Start a separate nested compositor, using the current desktop as its
parent display:

```sh
AIZOME_PROFILE_COMPOSITOR=./result/bin/aizome scripts/aizome-profile compositor
```

This opens a separate compositor window with a unique Wayland socket,
without a shell, session services or Xwayland. It does not replace the
current compositor. Close its window to finish the capture. To drive
repeatable headless workloads, choose your own socket and output:

```sh
scripts/aizome-profile heap ./result/bin/aizome \
  --backend headless --headless-output 1280x800@1 --socket aizome-heap \
  --no-session --no-shell --no-xwayland
```

Point test clients at `WAYLAND_DISPLAY=aizome-heap` and `aizomectl` at
`AIZOME_SOCKET=$XDG_RUNTIME_DIR/aizome.aizome-heap.sock`. Stop that isolated
compositor after the workload. To profile the shell, run it under Heaptrack
inside a separate headless session and D-Bus, so it cannot claim the live
desktop's notification service:

```sh
AIZOME_SESSION=0 dbus-run-session -- scripts/headless-session --no-shell --bin ./result/bin -- \
  scripts/aizome-profile heap ./result/bin/aizome-shell
```

The compositor and each child app are separate processes: capture the
process whose allocations you want to explain. A compositor trace does
not measure all its clients' heaps. For live-reload investigations, count
retained logic library mappings separately from allocations: old logic
code intentionally stays mapped after a reload.

When another agent or developer uses the machine, agree on a lock before
running timing benchmarks, for example `flock /tmp/aizome-benchmark.lock
COMMAND ...`. A lock only coordinates participants that honor it; check
for unrelated builds as well. Heap traces and process snapshots help find
causes, but performance conclusions need quiet, repeated uninstrumented
runs.

References: [Heaptrack's usage and Rust support](https://github.com/KDE/heaptrack),
[Nixpkgs split debug information](https://nixos.org/manual/nixpkgs/stable/#var-stdenv-separateDebugInfo),
[GDB separate debug files](https://sourceware.org/gdb/current/onlinedocs/gdb.html/Separate-Debug-Files.html).

## Compare allocators

`memory-allocator-build` snapshots the working tree and Sidestep, then
builds matching glibc and TiKV jemalloc versions of the shell, Soroban,
Sumi and Settei outside the checkout:

```sh
nix develop .#aizome --command scripts/memory-allocator-build /tmp/aizome-allocators 2
```

The experiment uses `tikv-jemallocator = 0.7.0` as Rust's global allocator,
with `override_allocator_on_supported_platforms`. This feature also keeps
the unprefixed C allocation entry points in the final link. The script
checks that the executables export them; a dedicated `LD_DEBUG=bindings`
run can verify that dependencies such as Fontconfig and FreeType bind to
those definitions. See the [TiKV allocator features](https://github.com/tikv/jemallocator/blob/main/jemalloc-sys/README.md).

It also makes `jemalloc/lib/libjemalloc.so` from the same compiled allocator
archive. Use that for a process-scoped compositor comparison; its Rust
`System` allocator reaches jemalloc through the intercepted C allocation
API, whereas the experimental apps use `Jemalloc` directly:

```sh
LD_PRELOAD=/tmp/aizome-allocators/jemalloc/lib/libjemalloc.so \
  aizome --backend headless --renderer pixman --headless-output 1280x800@1 \
  --socket aizome-allocator --no-session --no-shell --no-xwayland
```

Start with default settings. Measure PSS after the same workload and again
after 5, 10, 30 and 60 seconds idle. Jemalloc's background-thread support
is compiled in but disabled by default; idle applications may keep unused
pages until further allocation activity. A second arm with
`MALLOC_CONF=background_thread:true` tests asynchronous purging while
retaining the default decay time. Change decay times only in a separate
arm, checking the next interaction's latency as well as memory. The
[jemalloc tuning guide](https://github.com/jemalloc/jemalloc/blob/dev/TUNING.md)
explains the tradeoff between purging, arenas, CPU time and memory.

Include the existing allocator as a tuning control too. For example,
`MALLOC_MMAP_THRESHOLD_=131072 /tmp/aizome-allocators/glibc/aizome-shell`
fixes glibc's mmap threshold at 128 KiB for that process. This changes its
adaptive allocation policy and may trade buffer reuse for more mapping
calls. Measure the same workload, idle period and next interaction; keep
this arm separate from jemalloc's settings. See [glibc's allocator
tunables](https://sourceware.org/glibc/manual/latest/html_node/Memory-Allocation-Tunables.html).

Use uninstrumented process snapshots for these comparisons. Heaptrack's
usual C allocation interception does not necessarily observe Rust's direct
`mallocx`/`sdallocx` calls through `Jemalloc`; such a trace is not comparable
to a glibc trace. The experiment keeps jemalloc's optional allocation
statistics and profiling disabled for the primary memory/timing runs.
