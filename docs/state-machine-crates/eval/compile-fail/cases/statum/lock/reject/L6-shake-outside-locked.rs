// L6 shake-outside-locked: run the shake timer while the lock is `Locking`.
// The shake lives in `Locked` and `Checking`; `LockScreen<Locking>` has no
// `tick`.
use impl_statum::lock::{LockScreen, Unlocked};
use spec::lock::{Config, Effect};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let mut locking = LockScreen::<Unlocked>::new(Config::default()).lock(&mut out);
    locking.tick(&mut out);
}
