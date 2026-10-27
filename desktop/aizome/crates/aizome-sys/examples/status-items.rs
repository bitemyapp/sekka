//! Serve the status notifier watcher on this session's bus for a while and
//! show what registers: each item's properties and its menu.
//!
//! ```text
//! cargo run -p aizome-sys --example status-items [-- SECONDS]
//! ```
//!
//! Unlike `probe` it doesn't only read: it takes `org.kde.StatusNotifierWatcher`
//! (from the shell, which takes it back when it next starts) and reads each
//! item's menu (`AboutToShow`, `GetLayout`). It never clicks anything.

use std::time::{Duration, Instant};

use aizome_sys::Wake;
use aizome_sys::statusnotifier::{MenuItem, StatusNotifier};

fn main() {
    let seconds: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(5);
    let sn = StatusNotifier::new(Wake::none());
    if !sn.available() {
        eprintln!("couldn't serve the watcher");
        std::process::exit(1);
    }
    let end = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
    }
    let s = sn.snapshot();
    println!("{} items", s.items.len());
    for i in &s.items {
        println!("\n{}", i.key);
        println!("  id {:?}, title {:?}, label {:?}", i.id, i.title, i.label());
        println!("  category {:?}, status {:?}", i.category, i.status);
        let px: Vec<String> = i.icon.pixmaps.iter().map(|p| format!("{}x{}", p.width, p.height)).collect();
        println!("  icon {:?} pixmaps [{}], theme path {:?}", i.icon.name, px.join(" "), i.icon_theme_path);
        println!("  attention icon {:?} ({} pixmaps)", i.attention_icon.name, i.attention_icon.pixmaps.len());
        println!("  tooltip {:?} / {:?}", i.tooltip_title, i.tooltip_text);
        println!("  menu {:?}, item is menu {}", i.menu, i.item_is_menu);
        if i.menu.is_some() {
            match sn.menu(&i.key).wait() {
                Ok(root) => print_menu(&root.children, 2),
                Err(e) => println!("  menu: {e}"),
            }
        }
    }
}

fn print_menu(items: &[MenuItem], depth: usize) {
    for i in items {
        let pad = " ".repeat(depth * 2);
        if i.separator {
            println!("{pad}---");
        } else {
            let off = if i.enabled { "" } else { " (disabled)" };
            println!("{pad}{} [{}] {:?}{off}", i.label, i.id, i.toggle);
            print_menu(&i.children, depth + 1);
        }
    }
}
