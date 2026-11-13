// P5 hover-outside: a hovered node while the pointer is outside.
use impl_typestate::pointer::Outside;

fn main() {
    let outside = Outside::new();
    let _ = outside.hover();
}
