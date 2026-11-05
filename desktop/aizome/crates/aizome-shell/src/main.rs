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
/// free above 16 MB at its top. What frames allocate stays below both: a
/// band of the palette drawn ahead is 1 MB, the whole picture 5.6 MB.
/// See "Memory" under "Shell" in `docs/performance.md`.
fn fix_malloc_thresholds() {
    // Safety: mallopt only sets the allocator's parameters.
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::mallopt(libc::M_MMAP_THRESHOLD, 4 << 20);
        libc::mallopt(libc::M_TRIM_THRESHOLD, 16 << 20);
    }
}
