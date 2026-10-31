//! `aizome-hako [PATH…]`, `aizome-hako --select FILE`: Aizome's file
//! manager.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    #[cfg(feature = "gui")]
    if std::env::args().nth(1).as_deref() == Some("--file-manager-service") {
        hako::file_manager::serve()?;
    } else {
        hako::ui::main();
    }
    #[cfg(not(feature = "gui"))]
    eprintln!("aizome-hako was built without its window (the `gui` feature)");
    Ok(())
}
