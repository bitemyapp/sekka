//! What Tasuke recommends for a machine described in JSON (the shape
//! `tasuke hardware --json` prints): `cargo run --example recommend_for -- machine.json`.

use tasuke_models::{Catalog, Hardware, Preferences, Role, recommend};

fn main() {
    let path = std::env::args().nth(1).expect("a hardware JSON file");
    let hw: Hardware = serde_json::from_str(&std::fs::read_to_string(path).expect("readable")).expect("hardware JSON");
    let catalog = Catalog::load_default();
    for role in [Role::Assistant, Role::Desktop, Role::Fast] {
        println!("{}:", role.as_str());
        for r in recommend(&hw, &catalog, role, &Preferences::default()).iter().take(5) {
            println!("  {}{} {:?}  {}", if r.curated_default { "★ " } else { "" }, r.id.as_str(), r.fit, r.why);
        }
    }
}
