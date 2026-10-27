//! Files on the clipboard as file managers put them there: besides the
//! paths as text (what the kit's `Cx::set_clipboard_text` offers, for
//! terminals and editors), `text/uri-list` and GNOME's
//! `x-special/gnome-copied-files` (`copy` or `cut`, then the URIs), which
//! Nautilus, Nemo, Thunar, Dolphin and GTK's file choosers read to paste
//! files; and reading them back from another program's clipboard.
//!
//! sidestep-ui offers only text on the clipboard, so this speaks to
//! Sidestep's engine, whose selection already takes any types: after the
//! kit has set the text (and Sidestep has recorded the selection as the
//! program's own), the same selection is offered again with the file
//! types added. Without a display (tests) both halves do nothing and the
//! text stands alone. When sidestep-ui (and the kit) can put several
//! types on the clipboard, this goes through them instead.

use std::path::PathBuf;
use std::sync::Arc;

use sidestep_engine::clipboard::{self, Contents, GNOME_FILES, TEXT_MIMES, URI_LIST};

use crate::model::uri;

/// Offer `paths` (cut or copied) as files, alongside `text`, which the
/// kit has just put on the clipboard.
pub fn offer_files(paths: &[PathBuf], cut: bool, text: &str) {
    let text: Arc<[u8]> = Arc::from(text.as_bytes());
    let mut items: Vec<(String, Option<Arc<[u8]>>)> = vec![
        (GNOME_FILES.to_owned(), Some(Arc::from(uri::gnome_copied_files(cut, paths).into_bytes()))),
        (URI_LIST.to_owned(), Some(Arc::from(uri::uri_list(paths).into_bytes()))),
    ];
    items.extend(TEXT_MIMES.iter().map(|m| ((*m).to_owned(), Some(text.clone()))));
    clipboard::shared().offer(Some(Contents { items }));
}

/// Files another program put on the clipboard: whether they were cut,
/// and their paths. `None` if it offers no files.
pub fn foreign_files() -> Option<(bool, Vec<PathBuf>)> {
    let shared = clipboard::shared();
    let mimes = shared.foreign_mimes();
    log::debug!("clipboard offers {mimes:?}");
    if mimes.iter().any(|m| m == GNOME_FILES)
        && let Some(data) = shared.read_foreign(GNOME_FILES)
        && let Some(found) = uri::parse_gnome_copied_files(&String::from_utf8_lossy(&data))
        && !found.1.is_empty()
    {
        return Some(found);
    }
    if mimes.iter().any(|m| m == URI_LIST)
        && let Some(data) = shared.read_foreign(URI_LIST)
    {
        let files = uri::parse_uri_list(&String::from_utf8_lossy(&data));
        if !files.is_empty() {
            return Some((false, files));
        }
    }
    None
}
