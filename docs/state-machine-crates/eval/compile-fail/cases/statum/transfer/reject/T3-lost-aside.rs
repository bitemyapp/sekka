// T3 lost-aside: finish the item from `Restoring` without the restore's
// result. `Job<Restoring>` has one way out, `restored` (the Restored event),
// which reports a stranded aside; a `Job<Restoring>` isn't one of the asking
// states, whose `skip` moves on without a result.
use impl_statum::transfer::{AskingFiles, Job, Restoring};
use spec::transfer::Effect;

fn move_on(restoring: Job<Restoring>, out: &mut Vec<Effect>) {
    let _ = Job::<AskingFiles>::skip(restoring, out);
}

fn main() {}
