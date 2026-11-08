//! What the window shows, without the window: folders and their entries,
//! what kind of file each is, the places in the sidebar, thumbnails,
//! watching folders for changes, and `file://` URIs.

pub mod entry;
pub mod format;
pub use aizome_mime as mime;
pub mod places;
pub mod thumbs;
pub mod uri;
pub mod watch;
