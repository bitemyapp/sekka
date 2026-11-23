//! Signing in with the computer's account, for a forgotten PIN: a lock,
//! what it's for and whose account, and what went wrong, in the top half;
//! the keyboard (an overlay, its box dots) under them.

use aizome_kit::Icon;
use aizome_kit::layout::Point;
use aizome_kit::sidestep_ui::Canvas;
use aizome_kit::theme::{Role, Theme};

use super::W;
use super::look::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SignIn {
    pub title: String,
    pub lines: Vec<String>,
    /// What went wrong, or that it's being checked.
    pub error: Option<String>,
    pub checking: bool,
}

impl SignIn {
    pub fn paint(&self, c: &mut Canvas, theme: &Theme) {
        let mid = W / 2.0;
        icon(c, Icon::Lock, Point::new(mid, 150.0), 64.0, theme.role(Role::Warning));
        text_centred(c, theme, &self.title, Point::new(mid, 232.0), font(HEADLINE - 4.0, BOLD), theme.text());
        let mut y = 292.0;
        for l in &self.lines {
            text_centred(c, theme, l, Point::new(mid, y), font(BODY, REGULAR), theme.text2());
            y += 42.0;
        }
        let (said, ink) = match (&self.error, self.checking) {
            (_, true) => ("Checking…".to_owned(), theme.text3()),
            (Some(e), false) => (e.clone(), theme.role(Role::Danger)),
            (None, false) => (String::new(), theme.text3()),
        };
        if !said.is_empty() {
            text_centred(c, theme, &said, Point::new(mid, y + 14.0), font(BODY, MEDIUM), ink);
        }
    }
}
