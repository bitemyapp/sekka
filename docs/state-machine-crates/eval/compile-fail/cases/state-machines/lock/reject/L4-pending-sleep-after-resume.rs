// L4 pending-sleep-after-resume: the typed `resumed` from
// `Delay<SleepPending>` returns a `Delay<Held>`; the pending sleep can't
// be kept across it.
use impl_state_machines::lock::{Delay, DelayCtx, DelayHeldSleepOutcome, SleepPending};
use spec::lock::{Config, LockPhase};

fn main() {
    let held = Delay::new(DelayCtx::new(Config::default())).acquire().unwrap();
    let DelayHeldSleepOutcome::SleepPending(pending) = held.sleep(LockPhase::Locking).unwrap() else { return };
    let _still: Delay<SleepPending> = pending.resumed().unwrap();
}
