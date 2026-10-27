//! What Shashin asks of the window system beyond the kit's usual: full
//! screen.

use aizome_kit::prelude::*;

use crate::app::Msg;

/// Make `window` full screen (or not).
pub fn set_fullscreen(cx: &mut Cx<Msg>, window: WindowKey, on: bool) {
    cx.set_fullscreen(window, on);
}
