//! Aizome seen from the sofa: what a controller needs drawn, shared by
//! Chanoma (the television) and the login screen.
//!
//! - [`look`]: Aizome's look at twice its desktop size, and a controller's
//!   buttons drawn as the controller in hand prints them.
//! - [`keyboard`]: the on-screen keyboard: its keys, moving between them,
//!   what a press types and what each button does there, and the panel.
//! - [`pinpad`]: a PIN pressed on the controller's buttons, each standing
//!   for a digit, with chords for a controller short of buttons, and the
//!   pad that takes it.
//!
//! Everything is drawn on a 1920 × 1080 design ([`W`], [`H`]) that the
//! program scales to its surface with [`Fit`]: a television across a room
//! and a laptop at arm's length show the same panels at the same share of
//! the screen. The program owns what's typed and what it means (a page's
//! text box, a password, a PIN); these only show it and turn presses into
//! it.

pub mod keyboard;
pub mod look;
pub mod pinpad;

use aizome_kit::layout::{Point, Rect, Size};
use aizome_kit::sidestep_ui::Canvas;

/// The design's size, in its own points.
pub const W: f64 = 1920.0;
pub const H: f64 = 1080.0;

/// How the design sits in a surface: as large as fits, centred.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub scale: f64,
    pub origin: Point,
}

impl Fit {
    pub fn new(b: Rect) -> Fit {
        let scale = (b.width() / W).min(b.height() / H).max(0.01);
        Fit { scale, origin: Point::new(b.x0 + (b.width() - W * scale) / 2.0, b.y0 + (b.height() - H * scale) / 2.0) }
    }

    pub fn of_size(size: Size) -> Fit {
        Fit::new(Rect::from_origin_size(Point::ZERO, size))
    }

    pub fn apply(&self, c: &mut Canvas) {
        c.translate(self.origin.to_vec2());
        c.scale_by(self.scale, self.scale);
    }

    pub fn to_design(&self, p: Point) -> Point {
        Point::new((p.x - self.origin.x) / self.scale, (p.y - self.origin.y) / self.scale)
    }

    pub fn to_window(&self, r: Rect) -> Rect {
        Rect::new(
            self.origin.x + r.x0 * self.scale,
            self.origin.y + r.y0 * self.scale,
            self.origin.x + r.x1 * self.scale,
            self.origin.y + r.y1 * self.scale,
        )
    }

    /// The window's rectangle `b` in the design's points.
    pub fn visible(&self, b: Rect) -> Rect {
        let a = self.to_design(Point::new(b.x0, b.y0));
        let z = self.to_design(Point::new(b.x1, b.y1));
        Rect::new(a.x, a.y, z.x, z.y)
    }
}
