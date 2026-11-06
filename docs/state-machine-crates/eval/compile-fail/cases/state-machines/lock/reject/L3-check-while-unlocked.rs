// L3 check-while-unlocked: `submit` is declared only from `Locked`'s
// `Idle`; the typed `Locker<Locking>` has no such method.
use impl_state_machines::lock::{Locker, LockerCtx};
use spec::lock::Config;

fn main() {
    let locking = Locker::new(LockerCtx::new(Config::default())).lock().unwrap();
    let _ = locking.submit();
}
