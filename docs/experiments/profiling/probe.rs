// A symbol fixture, not an Aizome performance workload.
use std::hint::black_box;

#[inline(never)]
fn interaction_stage(mut value: u64) -> u64 {
    for i in 0..100_000 {
        value = black_box(value.wrapping_mul(6364136223846793005).wrapping_add(i));
    }
    value
}

fn main() {
    println!("{}", interaction_stage(black_box(42)));
}
