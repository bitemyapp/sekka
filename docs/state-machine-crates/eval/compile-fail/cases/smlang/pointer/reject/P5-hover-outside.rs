// P5 hover-outside: the hover is data of `Inside`'s substates only; the
// generated `States::Outside` (like `States::NativeDrag`) is a unit variant.
use impl_smlang::pointer::{Hover, States};

fn main() {
    let _ = States::Outside(Hover::default());
}
