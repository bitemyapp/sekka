// L3 check-while-unlocked. `Submit` is declared only for `Locked` (guarded
// by `has_text`); in `Unlocked`, `Locking` and `Checking` it is
// `Err(InvalidEvent)` (refused at run time).
use impl_smlang::lock::{Lock, LockError, LockEvents, LockStates};
use spec::lock::{Config, Effect, Machine};

fn main() {
    let mut m = Lock::new(Config::default());
    let mut out = Vec::new();
    let sm = &mut m;
    assert_eq!(sm.process_event(&mut out, LockEvents::Submit), Err(LockError::InvalidEvent));
    sm.process_event(&mut out, LockEvents::Lock).unwrap();
    assert_eq!(sm.process_event(&mut out, LockEvents::Submit), Err(LockError::InvalidEvent));
    sm.process_event(&mut out, LockEvents::LockConfirmed).unwrap();
    sm.process_event(&mut out, LockEvents::Typed(false)).unwrap();
    sm.process_event(&mut out, LockEvents::Submit).unwrap();
    assert!(matches!(sm.state_enum(), LockStates::Checking(_)));
    assert_eq!(sm.process_event(&mut out, LockEvents::Submit), Err(LockError::InvalidEvent));
    assert_eq!(out.iter().filter(|e| matches!(e, Effect::StartCheck { .. })).count(), 1);
}
