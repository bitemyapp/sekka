// Included only in the font test module. Allocation diagnostics are scoped to
// this thread; other native test/worker threads leave their counters disabled.
#[cfg(feature = "shape-allocation-probe")]
mod allocation_probe {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;
    thread_local! { static COUNT: Cell<Option<(u64, u64)>> = const { Cell::new(None) }; }
    pub struct Allocator;
    fn count(bytes: usize) {
        let _ = COUNT.try_with(|state| {
            if let Some((calls, total)) = state.get() {
                state.set(Some((calls.saturating_add(1), total.saturating_add(bytes as u64))));
            }
        });
    }
    // SAFETY: this test-only wrapper forwards each allocation and deallocation
    // to System unchanged. The constant-initialized TLS counter allocates no
    // memory and stores no allocation pointers. Realloc obeys System's contract.
    unsafe impl GlobalAlloc for Allocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            count(layout.size());
            unsafe { System.alloc(layout) }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            count(layout.size());
            unsafe { System.alloc_zeroed(layout) }
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) }
        }
        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            count(size);
            unsafe { System.realloc(pointer, layout, size) }
        }
    }
    pub fn start() { COUNT.with(|state| state.set(Some((0, 0)))); }
    pub fn stop() -> (u64, u64) { COUNT.with(|state| state.replace(None).unwrap()) }
}

#[cfg(not(feature = "shape-allocation-probe"))]
mod allocation_probe {
    pub fn start() {}
    pub fn stop() -> (u64, u64) { (0, 0) }
}

#[cfg(feature = "shape-allocation-probe")]
#[global_allocator]
static ALLOCATION_PROBE: allocation_probe::Allocator = allocation_probe::Allocator;

fn benchmark_jobs(cols: usize, rows: usize, kind: &str) -> Vec<Job> {
    let mut terminal = hotaru_core::Terminal::new(cols, rows, 0);
    terminal.set_default_grapheme_width(hotaru_core::GraphemeWidthMethod::Unicode);
    terminal.feed(b"\x1b[?25l");
    let text = if kind == "fallback" { "日本語 😀 👩\u{200d}💻 != a\u{301}  " } else { "abcdefgh != -> === 0123456789 " };
    for y in 0..rows {
        let line = text.repeat(cols / text.len() + 1);
        terminal.feed(format!("\x1b[{};1H{}", y + 1, line).as_bytes());
    }
    if kind == "cursor" { terminal.feed(b"\x1b[?25h"); }
    let mut jobs = Vec::new();
    let config = Arc::new(grid::FontConfig::default());
    for index in 0..160 {
        match kind {
            "unchanged" => (),
            "cell" | "fallback" => terminal.feed(format!("\x1b[{};{}H{}", index % rows + 1,
                index * 7 % cols + 1, char::from(b'A' + (index % 26) as u8)).as_bytes()),
            "cursor" => terminal.feed(format!("\x1b[{};{}H", index % rows + 1, index * 7 % cols + 1).as_bytes()),
            "scroll" => terminal.feed(format!("\x1b[{rows};1H\r\nline {index:08} {text}").as_bytes()),
            "replace" => {
                for y in 0..rows {
                    let fill = char::from(b'A' + ((index + y) % 26) as u8).to_string().repeat(cols);
                    terminal.feed(format!("\x1b[{};1H{fill}", y + 1).as_bytes());
                }
            }
            _ => unreachable!(),
        }
        jobs.push(Job { epoch: 0, snapshot: Arc::new(terminal.snapshot()), config: config.clone(),
            scale: 1.0, selection: None, search: None });
    }
    jobs
}

fn sample<T>(f: impl FnOnce() -> T) -> (T, u64, u64, u64) {
    allocation_probe::start();
    let start = std::time::Instant::now();
    let output = f();
    std::hint::black_box(&output);
    let elapsed = start.elapsed().as_nanos() as u64;
    let (calls, bytes) = allocation_probe::stop();
    (output, elapsed, calls, bytes)
}

#[test]
#[ignore = "serial worker preparation diagnostic; run only under shared workstation lock"]
fn benchmark_row_preparation() {
    install_fonts();
    let mut results = Vec::new();
    let cancel = AtomicBool::new(false);
    for (cols, rows) in [(120, 40), (240, 80)] {
        for kind in ["unchanged", "cell", "cursor", "scroll", "fallback", "replace"] {
            let jobs = benchmark_jobs(cols, rows, kind);
            let mut fonts = grid::FontSet::new((*jobs[0].config).clone()).unwrap();
            let mut cache = RowCache::default();
            // Every tested snapshot is checked against the preserved original
            // worker before timings. No missing visual work can count as speed.
            for work in &jobs { compared(work, &mut fonts, &mut cache); }
            let mut samples = Vec::new();
            for round in 0..4 {
                for cached in if round % 2 == 0 { [false, true] } else { [true, false] } {
                    let mut fonts = grid::FontSet::new((*jobs[0].config).clone()).unwrap();
                    let mut cache = RowCache::default();
                    let (mut nanoseconds, mut allocations, mut bytes) = (0u64, 0u64, 0u64);
                    let (mut prepared, mut reused, mut peak_cache) = (0usize, 0usize, 0usize);
                    let mut held_cached = None;
                    let mut held_uncached = None;
                    for (index, work) in jobs.iter().enumerate() {
                        let (elapsed, calls, allocated) = if cached {
                            let (frame, elapsed, calls, allocated) = sample(|| shape(work, &mut fonts, &cancel, &mut cache).unwrap());
                            held_cached = Some(frame); // Retire the preceding frame outside the measured interval.
                            (elapsed, calls, allocated)
                        } else {
                            let (frame, elapsed, calls, allocated) = sample(|| shape_uncached(work, &mut fonts, &cancel).unwrap());
                            held_uncached = Some(frame);
                            (elapsed, calls, allocated)
                        };
                        if index >= 32 {
                            nanoseconds += elapsed; allocations += calls; bytes += allocated;
                            prepared += if cached { cache.work.prepared } else { rows };
                            reused += cache.work.reused;
                            peak_cache = peak_cache.max(cache.work.retained_before + cache.work.retained_after);
                        }
                    }
                    std::hint::black_box((&held_cached, &held_uncached));
                    samples.push(serde_json::json!({"round":round,"cached":cached,"frames":128,
                        "nanoseconds":nanoseconds,"allocation_calls":allocations,"allocated_bytes":bytes,
                        "prepared_rows":prepared,"reused_rows":reused,"peak_cache_charge":peak_cache}));
                }
            }
            results.push(serde_json::json!({"cols":cols,"rows":rows,"workload":kind,"snapshots_compared":160,"samples":samples}));
        }
    }
    let report = serde_json::json!({"schema":1,"scope":"worker preparation only, no raster/presentation timings",
        "reference":"original uncached font::shape preserved in this test binary",
        "allocation_probe_enabled":cfg!(feature = "shape-allocation-probe"),
        "allocation_scope":"Rust global allocator calls on test thread during shape only; excludes frame retirement and C allocations",
        "row_cache_generation_limit":ROW_CACHE_BYTES,"results":results});
    let path = std::env::var("HOTARU_ROW_BENCH_OUTPUT").expect("set output file for evidence");
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
