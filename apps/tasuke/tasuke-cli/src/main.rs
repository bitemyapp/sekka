//! `tasuke`: the assistant's headless interface. See apps/tasuke/DESIGN.md.
//!
//! Subcommands live in their own modules: models.rs (hardware, recommend,
//! models, download, remove, use, server) and ask.rs (ask).

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        _ => {
            eprintln!("usage: tasuke hardware|recommend|models|download|remove|use|server|ask …");
            std::process::exit(2);
        }
    }
}
