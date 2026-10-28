//! Read-only diagnosis of the same completion advice Yukimi displays.
fn main() {
    let completion = yukimi_system::completion::read();
    println!("{}", serde_json::to_string_pretty(&completion).unwrap());
    println!("{}\n{}", completion.title(), completion.description());
}
