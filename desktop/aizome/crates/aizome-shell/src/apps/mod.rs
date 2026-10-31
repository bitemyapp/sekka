//! Apps: the ones connected to the shell socket with their menus, and the
//! installed ones from their desktop entries.

pub mod catalog;
pub mod registry;
pub mod server;
pub mod toolkit;

/// Exact application IDs, allowing a desktop filename and case differences.
pub(crate) fn same_app_id(a: &str, b: &str) -> bool {
    let a = a.trim().to_ascii_lowercase();
    let b = b.trim().to_ascii_lowercase();
    let a = a.strip_suffix(".desktop").unwrap_or(&a);
    let b = b.strip_suffix(".desktop").unwrap_or(&b);
    !a.is_empty() && a == b
}

#[cfg(test)]
pub(crate) fn test_index(entries: &[(&str, &str)]) -> std::sync::Arc<aizome_sys::apps::AppIndex> {
    use aizome_sys::apps::{AppIndex, Locale, XdgDirs};
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "aizome-shell-apps-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let dir = root.join("usr/share/applications");
    std::fs::create_dir_all(&dir).unwrap();
    for (id, extra) in entries {
        std::fs::write(
            dir.join(format!("{id}.desktop")),
            format!("[Desktop Entry]\nType=Application\nName={id}\nExec=/bin/false\n{extra}\n"),
        )
        .unwrap();
    }
    let index = AppIndex::load_with(XdgDirs::rooted(&root), Locale::default());
    std::fs::remove_dir_all(root).unwrap();
    std::sync::Arc::new(index)
}
