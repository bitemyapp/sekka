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
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    std::process::exit(aizome_shell::run());
}
