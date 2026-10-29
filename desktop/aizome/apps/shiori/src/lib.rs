//! Shiori 栞 (Documents), Aizome's PDF reader: pages in one column,
//! zoomed to fit or by steps, pinches and ❖-scrolls; the pages' text to
//! select and copy; find; links and the document's outline; a sidebar of
//! pages; your place in each document kept. hayro draws the pages, and
//! reads their text and the document's structure, in a process of its own
//! for each document (`worker`), so a file that panics it or keeps it busy
//! costs a page, not the window; tiles are drawn for exactly the zoom and
//! the screen's scale, and copied one to one.

pub mod app;
pub mod document;
pub mod engine;
pub mod fonts;
pub mod instance;
pub mod layout;
pub mod pages;
pub mod pdftext;
pub mod places;
pub mod protocol;
pub mod text;
pub mod ui;
pub mod worker;

pub use app::{Msg, Shiori};
