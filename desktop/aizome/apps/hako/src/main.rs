//! `aizome-hako [PATH…]`, `aizome-hako --select FILE`: Aizome's file
//! manager.

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    #[cfg(feature = "gui")]
    hako::ui::main();
    #[cfg(not(feature = "gui"))]
    eprintln!("aizome-hako was built without its window (the `gui` feature)");
}
