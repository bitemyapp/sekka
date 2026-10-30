//! `cargo run -p aizome-nisshi --example sample -- DIR`: make DIR a
//! stand-in journal and /var/log holding the made-up boot
//! ([`nisshi::fixture::stage`]), and say how to open Nisshi on it, to try
//! it (or picture it) without this computer's messages.

fn main() {
    let Some(dir) = std::env::args().nth(1).map(std::path::PathBuf::from) else {
        eprintln!("Usage: sample DIR");
        std::process::exit(2);
    };
    if let Err(e) = nisshi::fixture::stage(&dir) {
        eprintln!("{}: {e}", dir.display());
        std::process::exit(1);
    }
    let dir = dir.canonicalize().unwrap_or(dir);
    println!("NISSHI_JOURNALCTL={}/journalctl NISSHI_LOG_DIR={}/log aizome-nisshi", dir.display(), dir.display());
}
