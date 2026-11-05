//! How the policy classifies a command: `cargo run --example classify -- HOME 'COMMAND'`.
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let home = std::path::Path::new(&args[0]);
    for cmd in &args[1..] {
        println!("{cmd}\n  as the person: {:?}\n  refused: {:?}", tasuke_agent::policy::classify_command(cmd, home),
                 tasuke_agent::policy::refused(cmd, home));
        println!("  refused from / for /root: {:?}", tasuke_agent::policy::refused_from(cmd, std::path::Path::new("/root"), std::path::Path::new("/")));
    }
}
