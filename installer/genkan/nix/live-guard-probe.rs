// Diagnostic VM entrypoint only; never installed in the live image.
use calamares_nixos::{Kernel, Settings, install};

fn main() {
    let settings = Settings {
        template_dir: "/unused".into(),
        zoneinfo: "/unused".into(),
        state_version: "26.11".into(),
        kernel: Kernel::Latest,
        test_diagnostics: false,
    };
    if let Err(error) = install::live_guard(&settings) {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
    println!("Live media guard accepted");
}
