// L6 shake-outside-locked: a running shake while locking. The shake counter
// (the timer runs while it's > 0) is data of `Phase::Locked` and
// `Phase::Checking` only.
use impl_enum::lock::Phase;

fn main() {
    let _ = Phase::Locking { shake: 1 };
}
