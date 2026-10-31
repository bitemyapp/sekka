#[path = "../src/math.rs"]
mod math;
use std::io::{self, BufRead, Write};
fn main() -> io::Result<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let line = line?;
        let bits: Vec<u64> = line
            .split_whitespace()
            .map(|x| u64::from_str_radix(x, 16))
            .collect::<Result<_, _>>()
            .map_err(io::Error::other)?;
        if bits.len() != 2 {
            return Err(io::Error::other("expected two hex bit patterns"));
        }
        let x = f64::from_bits(bits[0]);
        let y = f64::from_bits(bits[1]);
        writeln!(
            out,
            "{:016x} {:016x}",
            math::hypot(x, y).to_bits(),
            x.hypot(y).to_bits()
        )?;
    }
    out.flush()
}
