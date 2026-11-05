// T3 lost-aside: finish the item from `Restoring` without the restore's
// result. `Restoring` has one way out, `restored`, which reports a stranded
// aside. (Rust can't force the value to be used: it can be dropped, but
// nothing then yields a next state.)
use impl_typestate::transfer::{Any, Job, Restoring};
use spec::transfer::{Effect, Outcome};

fn move_on(job: &mut Job, restoring: Restoring, out: &mut Vec<Effect>) -> Any {
    restoring.done(job, Outcome::Failed, out)
}

fn main() {}
