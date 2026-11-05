// L7 double-delay: `acquire` (SessionStarted, LogindAppeared) is declared
// only from `NotHeld`; the typed `Delay<Held>` has no such method.
use impl_state_machines::lock::{Delay, DelayCtx};
use spec::lock::Config;

fn main() {
    let held = Delay::new(DelayCtx::new(Config::default())).acquire().unwrap();
    let _ = held.acquire();
}
