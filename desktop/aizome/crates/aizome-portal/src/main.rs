use std::io::Read;

use aizome_portal::chooser::Chooser;
use aizome_portal::memory::Memory;
use aizome_portal::request::Request;

/// The most a request may be: thousands of filters fit.
const MAX_REQUEST: usize = 4 << 20;

fn chooser() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::io::stdin().take((MAX_REQUEST + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > MAX_REQUEST {
        return Err("the file chooser's request is too large".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    let chooser = aizome_kit::try_run(Chooser::new(request, Memory::path()))?;
    // `null` for cancelled.
    serde_json::to_writer(std::io::stdout(), &chooser.answer)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    match std::env::args().nth(1).as_deref() {
        None => aizome_portal::service::serve(),
        Some("--file-chooser") => chooser(),
        Some("--help" | "-h") => {
            println!(
                "aizome-portal: Aizome's xdg-desktop-portal backend (file choosers).\n\n\
                 Usage: aizome-portal                 serve {} on the session bus\n       \
                 aizome-portal --file-chooser  show one chooser: a request as JSON on standard input,\n                                     \
                 the answer as JSON on standard output",
                aizome_portal::BUS_NAME
            );
            Ok(())
        }
        Some(other) => Err(format!("unknown argument “{other}”").into()),
    }
}
