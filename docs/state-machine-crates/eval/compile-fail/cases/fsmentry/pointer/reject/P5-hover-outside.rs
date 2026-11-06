// P5 hover-outside: set the hover while `Outside`. The hover is `Inside`'s
// data; `Outside` is a unit node, so its handle has no `as_mut`.
use impl_fsmentry::pointer::{Over, Pointer, WindowEntry};
use spec::pointer::{Config, Machine};

fn main() {
    let mut m = Pointer::new(Config::default());
    if let WindowEntry::Outside(mut outside) = m.entry() {
        let _hover: &mut Over = outside.as_mut();
    }
}
