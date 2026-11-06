// L6 shake-outside-locked: `shake_tick` is declared from `Locked` (whose
// data is the shake); the typed `Locker<Locking>` has no such method.
use impl_state_machines::lock::{Locker, LockerCtx};
use spec::lock::Config;

fn main() {
    let locking = Locker::new(LockerCtx::new(Config::default())).lock().unwrap();
    let _ = locking.shake_tick();
}
