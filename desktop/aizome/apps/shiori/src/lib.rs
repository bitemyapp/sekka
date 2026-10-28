//! Shiori 栞 (Documents), Aizome's PDF reader: pages in one column,
//! zoomed to fit or by steps, pinches and ❖-scrolls; the pages' text to
//! select and copy; find; links and the document's outline; a sidebar of
//! pages; your place in each document kept. PDFium draws the pages in a
//! process of its own for each document (`worker`), so a file that
//! crashes or hangs it costs a page, not the window; tiles are drawn for
//! exactly the zoom and the screen's scale, and copied one to one.

pub mod app;
pub mod engine;
pub mod instance;
pub mod layout;
pub mod pages;
pub mod places;
pub mod protocol;
pub mod text;
pub mod ui;
pub mod worker;

pub use app::{Msg, Shiori};
