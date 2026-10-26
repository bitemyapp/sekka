//! Copying a picture: offered on the clipboard as `image/png`, through
//! the clipboard Sidestep's engine keeps (its toolkit offers only text).

use std::sync::Arc;

use sidestep_engine::clipboard::{self, Contents};

/// Offer `png` as the clipboard's contents, for this program and others.
/// It takes once a window has had input (Wayland's rule), as any copy.
pub fn offer_png(png: Vec<u8>) {
    let shared = clipboard::shared();
    shared.bump();
    let data: Arc<[u8]> = png.into();
    shared.offer(Some(Contents { items: vec![("image/png".to_owned(), Some(data))] }));
}

/// A picture as PNG, from straight RGBA.
pub fn encode_png(img: &image::RgbaImage) -> Option<Vec<u8>> {
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}
