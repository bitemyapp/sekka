// L3 check-while-unlocked: start a PAM check while the lock is `Locking`.
// Only `Locked` (the idle lock screen) has `submit`.
use impl_typestate::lock::Unlocked;
use spec::lock::Effect;

fn main() {
    let mut out: Vec<Effect> = Vec::new();
    let mut last_check = 0;
    let locking = Unlocked::new().lock(0b1, &mut out);
    let _ = locking.submit(&mut last_check, &mut out);
}
