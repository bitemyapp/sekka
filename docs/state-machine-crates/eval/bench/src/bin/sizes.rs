//! `size_of` of every implemented machine type, as TSV
//! (`implementation target bytes`), plus the spec's event and effect types.
//! `cargo run --release -p bench --bin sizes`

use bench::impls;

macro_rules! sizes {
    ($($name:literal => $krate:ident),* $(,)?) => {
        fn print() {
            macro_rules! one {
                ($t:ident, $Type:ident) => {
                    $(
                        if bench::$krate::$t::IMPLEMENTED {
                            println!("{}\t{}\t{}", $name, stringify!($t), std::mem::size_of::<bench::$krate::$t::$Type>());
                        }
                    )*
                    println!("spec-event\t{}\t{}", stringify!($t), std::mem::size_of::<spec::$t::Event>());
                    println!("spec-effect\t{}\t{}", stringify!($t), std::mem::size_of::<spec::$t::Effect>());
                };
            }
            bench::targets!(one);
        }
    };
}

impls!(sizes);

fn main() {
    println!("implementation\ttarget\tbytes");
    print();
}
