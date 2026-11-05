// K4 double-reply: reply to polkitd, then reply again. Every way out of a
// request (`succeed`, `end`) consumes the state that holds it.
use impl_typestate::polkit::Prompted;
use spec::polkit::Effect;

fn reply_twice(prompted: Prompted, out: &mut Vec<Effect>) {
    let _ = prompted.succeed(out);
    let _ = prompted.end(out);
}

fn main() {}
