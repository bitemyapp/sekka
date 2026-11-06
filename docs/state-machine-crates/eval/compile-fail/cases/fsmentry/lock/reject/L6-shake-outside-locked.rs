// L6 shake-outside-locked: start the shake while `Locking`. The shake is
// `Locked`'s data; `Locking` is a unit node, so its handle has no `as_mut`.
use impl_fsmentry::lock::{LockedData, Phase, PhaseEntry};

fn main() {
    let mut phase = Phase::Locking;
    if let PhaseEntry::Locking(mut locking) = phase.entry() {
        let _locked: &mut LockedData = locking.as_mut();
    }
}
