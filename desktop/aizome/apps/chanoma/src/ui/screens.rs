//! Screens that say one thing: a picture, a few words, and big buttons
//! (time's up, bedtime, the welcome, a game starting, pairing a
//! controller, setting its buttons up).

use aizome_kit::layout::{Point, Rect, Vec2};
use aizome_kit::sidestep_ui::kurbo::{Circle, RoundedRect};
use aizome_kit::sidestep_ui::{Canvas, Color, StrokeStyle};
use aizome_kit::theme::Theme;

use super::look::*;
use super::{Focus, H, W};
use crate::pad::{Button, Family};

/// The picture at the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Art {
    /// Bedtime: the moon and stars.
    Moon,
    /// Time's up.
    Hourglass,
    /// Turn a controller on; pair one.
    Controller,
    /// The welcome: a lamp-lit room's window.
    Lamp,
    /// Something is starting.
    Waiting,
    /// Something went wrong.
    Oops,
    /// Press this button (setting a controller's buttons up): drawn as
    /// the controller's family prints it.
    Press(Button, Family),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Message {
    pub art: Art,
    pub title: String,
    pub body: String,
    /// A list under the words (controllers found, …).
    pub lines: Vec<String>,
    pub actions: Vec<String>,
}

impl Message {
    pub fn new(art: Art, title: impl Into<String>, body: impl Into<String>) -> Message {
        Message { art, title: title.into(), body: body.into(), lines: Vec::new(), actions: Vec::new() }
    }

    pub fn actions(mut self, a: &[&str]) -> Message {
        self.actions = a.iter().map(|s| (*s).to_owned()).collect();
        self
    }

    /// Where the words and the list end: the words' lines estimated
    /// generously (a hit test has no fonts to measure with).
    fn body_bottom(&self) -> f64 {
        let lines = (self.body.chars().count() as f64 / 58.0).ceil().max(1.0);
        572.0 + 45.0 * lines + 52.0 * self.lines.len().min(6) as f64
    }

    fn action_rect(&self, i: usize) -> Rect {
        let n = self.actions.len().max(1) as f64;
        let w = 520.0;
        let gap = 36.0;
        let total = n * w + (n - 1.0) * gap;
        let x = W / 2.0 - total / 2.0 + i as f64 * (w + gap);
        let y = (self.body_bottom() + 56.0).min(H - 230.0);
        Rect::new(x, y, x + w, y + 104.0)
    }

    pub fn hit(&self, p: Point) -> Option<usize> {
        (0..self.actions.len()).find(|i| self.action_rect(*i).contains(p))
    }

    pub fn paint(&self, c: &mut Canvas, theme: &Theme, focus: Focus) {
        let at = Point::new(W / 2.0, 300.0);
        match self.art {
            Art::Moon => {
                for (i, (dx, dy, r)) in [
                    (-260.0, -60.0, 5.0),
                    (240.0, -90.0, 4.0),
                    (300.0, 40.0, 6.0),
                    (-200.0, 70.0, 3.5),
                    (-330.0, 10.0, 4.5),
                    (160.0, 100.0, 3.0),
                ]
                .iter()
                .enumerate()
                {
                    let a = 0.55 + (i % 3) as f32 * 0.15;
                    c.fill(&Circle::new(at + Vec2::new(*dx, *dy), *r), Color::rgba(1.0, 0.96, 0.85, a));
                }
                c.fill(&Circle::new(at, 150.0), Color::rgba(1.0, 0.86, 0.55, 0.08));
                // The bite out of it is the sky behind, glow and all.
                let sky = BG_TOP.mix(BG_BOTTOM, 0.25).mix(Color::hex(0xffdb8c), 0.08);
                moon(c, at, 92.0, Color::hex(0xfff1cf), sky);
            }
            Art::Hourglass => {
                c.fill(&Circle::new(at, 150.0), Color::rgba(1.0, 0.78, 0.35, 0.10));
                hourglass(c, at, 190.0, WARN);
            }
            Art::Controller => {
                c.fill(&Circle::new(at, 160.0), Color::rgba(0.48, 0.55, 1.0, 0.10));
                controller(c, at, 290.0, INK, Color::hex(0x2a3270));
            }
            Art::Lamp => {
                c.fill(&Circle::new(at, 170.0), Color::rgba(1.0, 0.75, 0.35, 0.12));
                let pane = Rect::from_center_size(at, aizome_kit::layout::Size::new(250.0, 210.0));
                c.fill(&RoundedRect::from_rect(pane, 14.0), Color::hex(0xffcf7a));
                let s = StrokeStyle::new(14.0);
                c.stroke(&RoundedRect::from_rect(pane, 14.0), &s, Color::hex(0x5a3a1c));
                c.stroke(
                    &aizome_kit::sidestep_ui::kurbo::Line::new((at.x, pane.y0), (at.x, pane.y1)),
                    &s,
                    Color::hex(0x5a3a1c),
                );
                c.stroke(
                    &aizome_kit::sidestep_ui::kurbo::Line::new((pane.x0, at.y), (pane.x1, at.y)),
                    &s,
                    Color::hex(0x5a3a1c),
                );
            }
            Art::Waiting => {
                for i in 0..3 {
                    c.fill(&Circle::new(at + Vec2::new((f64::from(i) - 1.0) * 70.0, 0.0), 22.0), INK_SOFT);
                }
            }
            Art::Oops => {
                c.fill(&Circle::new(at, 130.0), Color::rgba(1.0, 0.42, 0.36, 0.12));
                text_centred(c, theme, "!", at, font(170.0, 900.0), DANGER);
            }
            Art::Press(b, family) => {
                c.fill(&Circle::new(at, 150.0), Color::rgba(1.0, 0.71, 0.24, 0.10));
                button_glyph(c, theme, family, b, at, 96.0);
            }
        }
        text_centred(c, theme, &self.title, Point::new(W / 2.0, 520.0), font(68.0, 900.0), INK);
        let mut y = paragraph(c, theme, &self.body, W / 2.0, 572.0, 1240.0, font(34.0, 600.0), INK_SOFT);
        for l in self.lines.iter().take(6) {
            text_centred(c, theme, l, Point::new(W / 2.0, y + 32.0), font(32.0, 700.0), INK);
            y += 52.0;
        }
        for (i, a) in self.actions.iter().enumerate() {
            let r = self.action_rect(i);
            let on = focus.row == i;
            c.fill(&RoundedRect::from_rect(r, 30.0), if on { INK } else { Color::hex(0x2c3474) });
            text_centred(c, theme, a, r.center(), font(36.0, 800.0), if on { Color::hex(0x141a3a) } else { INK });
            if on {
                focus_ring(c, r, 30.0);
            }
        }
    }
}
