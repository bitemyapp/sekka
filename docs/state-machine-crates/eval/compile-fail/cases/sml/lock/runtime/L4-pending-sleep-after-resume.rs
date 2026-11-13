// L4 pending-sleep-after-resume. `SleepPending + Resumed = Held` (no action,
// so no second `TakeDelay`), and the later confirmation has no `Held` row in
// the delay region: the delay protects the next sleep. Refused at run time.
use impl_sml::lock::{Lock, LockEvents as E};
use spec::lock::{Config, DelayPhase, Effect, LockPhase, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out: Vec<Effect> = Vec::new();
    m.machine.process_event(&mut out, E::SessionStarted).unwrap();
    m.machine.process_event(&mut out, E::PrepareForSleep).unwrap();
    assert_eq!(m.state().delay, DelayPhase::SleepPending);
    m.machine.process_event(&mut out, E::Resumed).unwrap();
    assert_eq!(m.state().delay, DelayPhase::Held);
    m.machine.process_event(&mut out, E::LockConfirmed).unwrap();
    assert_eq!(m.state().lock, LockPhase::Locked);
    assert_eq!(m.state().delay, DelayPhase::Held);
    assert!(!out.contains(&Effect::ReleaseDelay));
    assert_eq!(out.iter().filter(|e| **e == Effect::TakeDelay).count(), 1);
}
