// L6 shake-outside-locked: a running shake while locking. The shake counter
// (the timer runs while it's > 0) is the state-local storage of the `locked`
// superstate: only its leaves `Idle` and `Checking` carry it.
use impl_statig::lock::State;

fn main() {
    let _ = State::Locking { shake: 1 };
}
