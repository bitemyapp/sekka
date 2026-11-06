// L6 shake-outside-locked: run the shake timer while the lock is `Locking`.
// The shake lives in `Locked` and `Checking`; `Locking` has no `tick`.
use impl_typestate::lock::Unlocked;
use spec::lock::Effect;

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let locking = Unlocked::new().lock(0b1, &mut out);
    locking.tick(&mut out);
}
