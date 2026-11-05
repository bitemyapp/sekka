//! Print what Tasuke detects on this machine, and how long detection took.
fn main() {
    let start = std::time::Instant::now();
    let hw = tasuke_models::Hardware::detect();
    let took = start.elapsed();
    println!("{}", serde_json::to_string_pretty(&hw).unwrap());
    eprintln!("detected in {took:?}");
}
