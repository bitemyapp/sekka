//! The desktop entry is also Utsushi's format declaration for MPRIS and
//! folder expansion. Decoding remains the media backend's responsibility.
use aizome_apps::{DesktopEntry, Locale};
use std::sync::OnceLock;

pub fn desktop_entry() -> &'static DesktopEntry {
    static ENTRY: OnceLock<DesktopEntry> = OnceLock::new();
    ENTRY.get_or_init(|| {
        DesktopEntry::parse(
            include_str!("../data/org.aizome.Utsushi.desktop"),
            "org.aizome.Utsushi.desktop",
            &Locale::untranslated(),
        )
        .expect("Utsushi's bundled desktop entry")
    })
}
