//! `aizome-shell`: Aizome's desktop shell. See `docs/shell.md`.

fn main() {
    let mut args = std::env::args().skip(1);
    if let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                println!("{}", aizome_shell::USAGE);
                return;
            }
            "-V" | "--version" => {
                println!("aizome-shell {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            "msg" => std::process::exit(aizome_shell::msg(args.collect())),
            other => {
                eprintln!("aizome-shell: unknown option {other}\n\n{}", aizome_shell::USAGE);
                std::process::exit(2);
            }
        }
    }
    fix_malloc_thresholds();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    std::process::exit(aizome_shell::run());
}

/// Fix glibc's thresholds before any thread starts. Left alone, glibc
/// raises them to the largest block it has unmapped (a 4K frame, 32 MB),
/// and from then on each thread's arena keeps up to 64 MB of freed memory
/// at its top for good: the threads that decode wallpapers and read tray
/// icons and D-Bus replies held hundreds of megabytes that way, nine tenths
/// of it free, which swapping wrote out and reuse read back. With them
/// fixed, blocks of 4 MB and up are mapped on their own and unmapped when
/// freed (zeroed memory then comes free, too), and an arena returns what's
/// free above 16 MB at its top. A live window draws into its shared-memory
/// pool, so the biggest block a frame takes from the heap, a band of the
/// palette drawn ahead, is 1 MB, below both. A threshold set in the
/// environment (GLIBC_TUNABLES, or its MALLOC_*_ alias) is left as set.
/// See "Memory" under "Shell" in `docs/performance.md`.
fn fix_malloc_thresholds() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        let tunables = std::env::var("GLIBC_TUNABLES").unwrap_or_default();
        let set = |tunable: &str, alias: &str| {
            std::env::var_os(alias).is_some()
                || tunables.split(':').any(|t| t.split_once('=').is_some_and(|(name, _)| name == tunable))
        };
        // Safety: mallopt only sets the allocator's parameters.
        unsafe {
            if !set("glibc.malloc.mmap_threshold", "MALLOC_MMAP_THRESHOLD_") {
                libc::mallopt(libc::M_MMAP_THRESHOLD, 4 << 20);
            }
            if !set("glibc.malloc.trim_threshold", "MALLOC_TRIM_THRESHOLD_") {
                libc::mallopt(libc::M_TRIM_THRESHOLD, 16 << 20);
            }
        }
    }
}
