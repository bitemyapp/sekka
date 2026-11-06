// L3 check-while-unlocked: start a PAM check while the lock is `Locking`.
// Only `LockScreen<Locked>` (the idle lock screen, reached by `confirm`) has
// `submit`.
use impl_statum::lock::{LockScreen, Unlocked};
use spec::lock::{Config, Effect};

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let locking = LockScreen::<Unlocked>::new(Config::default()).lock(&mut out);
    let _ = locking.submit(&mut out);
}
