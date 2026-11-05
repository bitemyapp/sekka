// L6 shake-outside-locked: a running shake while locking. The shake
// counter is data of `Locked` (`Field`) and `Checking` (`Check`) only; the
// generated `LockStates::Locking` is a unit variant.
use impl_smlang::lock::{Field, LockStates};

fn main() {
    let _ = LockStates::Locking(Field { has_text: false, shake: 1 });
}
