// K4 double-reply: reply to polkitd, then reply again. Every way out of a
// request (`succeed`, `end`) consumes the state that holds it.
use impl_statum::polkit::{Prompted, Session};
use spec::polkit::Effect;

fn reply_twice(prompted: Session<Prompted>, out: &mut Vec<Effect>) {
    let _ = prompted.succeed(out);
    let _ = prompted.end(out);
}

fn main() {}
