//! The system bell (the compositor's `bell` request, for xdg-system-bell):
//! the alert sound (`[sound] alert_sound`, at `alert_volume`) and, with
//! `visual_alert`, a flash of the screen's edge as well, a frame of
//! `amber` on an overlay surface that takes no input, gone after a moment.

use std::time::Duration;

use aizome_kit::element::Element;
use aizome_kit::prelude::*;
use aizome_kit::{Anchors, KeyboardInteractivity, Layer, LayerOptions};

use crate::shell::{Msg, Shell, Surface, layer};
use crate::widgets::drawn::drawn;

/// How long the edge shows.
pub const FLASH: Duration = Duration::from_millis(180);
/// How wide the edge is.
pub const EDGE: f64 = 6.0;

impl Shell {
    /// The bell rang on `output` (else the focused one).
    pub fn bell(&mut self, output: Option<&str>, cx: &mut Cx<Msg>) {
        self.alert_sound();
        if !self.sound_cfg.visual_alert {
            return;
        }
        let Some(screen) = output.and_then(|n| self.screen_named(n)).or(self.focused_screen()).cloned() else { return };
        self.end_flash(cx);
        let opts = LayerOptions::new(Layer::Overlay, "aizome-flash")
            .anchors(Anchors::TOP | Anchors::BOTTOM | Anchors::LEFT | Anchors::RIGHT)
            .exclusive_zone(-1)
            .keyboard(KeyboardInteractivity::None)
            .output(screen.output.id);
        let size = screen.output.frame.size();
        let w = cx.open_window(layer(opts, size.width, size.height, "Flash"));
        // It takes no clicks: everything under it stays reachable.
        cx.set_input_region(w, Some(Vec::new()));
        self.surfaces.insert(w, Surface::Flash);
        self.serial += 1;
        self.flash = Some((w, self.serial));
        cx.after(FLASH, Msg::FlashDone(self.serial));
    }

    pub fn flash_done(&mut self, n: u64, cx: &mut Cx<Msg>) {
        if self.flash.is_some_and(|(_, s)| s == n) {
            self.end_flash(cx);
        }
    }

    fn end_flash(&mut self, cx: &mut Cx<Msg>) {
        if let Some((w, _)) = self.flash.take() {
            cx.close_window(w);
            self.surfaces.remove(&w);
        }
    }
}

/// The flash: the edge, and nothing inside it.
pub fn view() -> Element<Msg> {
    drawn(0.0, 0.0, "flash", |cx| {
        let t = cx.theme();
        let b = cx.bounds();
        let c = crate::look::Hue::Amber.color(t);
        cx.canvas.fill_rect(Rect::new(0.0, 0.0, b.x1, EDGE), c);
        cx.canvas.fill_rect(Rect::new(0.0, b.y1 - EDGE, b.x1, b.y1), c);
        cx.canvas.fill_rect(Rect::new(0.0, EDGE, EDGE, b.y1 - EDGE), c);
        cx.canvas.fill_rect(Rect::new(b.x1 - EDGE, EDGE, b.x1, b.y1 - EDGE), c);
    })
    .into()
}
