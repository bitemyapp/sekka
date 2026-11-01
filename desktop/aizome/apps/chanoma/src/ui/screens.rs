//! Screens that say one thing (time's up, bedtime, the welcome, a game
//! starting, pairing a controller, setting its buttons up), laid out as
//! Aizome's lock screen is: one picture, a line, a few quieter words under
//! it, and buttons.

use aizome_kit::Icon;
use aizome_kit::layout::{Point, Rect};
use aizome_kit::sidestep_ui::Canvas;
use aizome_kit::theme::{Role, Theme};

use super::look::*;
use super::{Focus, H, W};
use crate::pad::{Button, Family};

/// The picture at the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Art {
    /// Bedtime.
    Moon,
    /// Time's up.
    Hourglass,
    /// Turn a controller on; pair one.
    Controller,
    /// The welcome: Aizome's mark.
    Mark,
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

const ART_Y: f64 = 300.0;
const TITLE_Y: f64 = 450.0;
const BODY_TOP: f64 = 494.0;
const BODY_W: f64 = 1080.0;
const LINE_H: f64 = 50.0;

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
        let lines = if self.body.is_empty() { 0.0 } else { (self.body.chars().count() as f64 / 64.0).ceil() };
        BODY_TOP + 42.0 * lines + LINE_H * self.lines.len().min(6) as f64
    }

    fn action_rect(&self, i: usize) -> Rect {
        let n = self.actions.len().max(1) as f64;
        let w = 440.0;
        let gap = 32.0;
        let total = n * w + (n - 1.0) * gap;
        let x = (W / 2.0 - total / 2.0 + i as f64 * (w + gap)).round();
        let y = (self.body_bottom() + 64.0).min(H - 230.0).round();
        Rect::new(x, y, x + w, y + 88.0)
    }

    pub fn hit(&self, p: Point) -> Option<usize> {
        (0..self.actions.len()).find(|i| self.action_rect(*i).contains(p))
    }

    pub fn paint(&self, c: &mut Canvas, theme: &Theme, focus: Focus) {
        let at = Point::new(W / 2.0, ART_Y);
        match self.art {
            Art::Moon => icon(c, Icon::Moon, at, 120.0, theme.text2()),
            Art::Hourglass => icon(c, Icon::Hourglass, at, 120.0, theme.role(Role::Warning)),
            Art::Controller => icon(c, Icon::Gamepad2, at, 136.0, theme.text2()),
            Art::Mark => mark(c, theme, Point::new(at.x - 48.0, at.y - 48.0), 24.0),
            Art::Waiting => {
                // Three squares, still: Aizome doesn't spin.
                for i in 0..3 {
                    let x = at.x + (f64::from(i) - 1.0) * 40.0;
                    c.fill_rect(Rect::from_center_size(Point::new(x, at.y), (14.0, 14.0)), theme.text3());
                }
            }
            Art::Oops => icon(c, Icon::TriangleAlert, at, 120.0, theme.role(Role::Danger)),
            Art::Press(b, family) => button_glyph(c, theme, family, b, at, 80.0),
        }
        let title = fit_text(theme, &self.title, W - 2.0 * 160.0, font(HEADLINE + 8.0, BOLD));
        text_centred(c, theme, &title, Point::new(W / 2.0, TITLE_Y), font(HEADLINE + 8.0, BOLD), theme.text());
        let mut y =
            paragraph(c, theme, &self.body, W / 2.0, BODY_TOP, BODY_W, font(BODY + 2.0, REGULAR), theme.text2());
        for l in self.lines.iter().take(6) {
            text_centred(c, theme, l, Point::new(W / 2.0, y + LINE_H / 2.0 + 8.0), font(BODY, MEDIUM), theme.text());
            y += LINE_H;
        }
        for (i, a) in self.actions.iter().enumerate() {
            button(c, theme, self.action_rect(i), a, focus.row == i);
        }
    }
}
