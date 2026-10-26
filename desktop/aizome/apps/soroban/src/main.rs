//! `aizome-soroban`: the calculator.

use aizome_soroban::app::{Soroban, state_path};
use aizome_soroban::instance::{self, Claim};

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut app = Soroban::new(Some(state_path()));
    match instance::claim("soroban", &args) {
        Claim::Handed => return,
        Claim::First(listener) => app.instance = Some(listener),
        Claim::Alone => {}
    }
    aizome_kit::run(app);
}
