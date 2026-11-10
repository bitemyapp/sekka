//! What a controller brings up over the login screen, drawn as Chanoma
//! draws for the sofa (`aizome_sofa`) on its 1920 × 1080 design scaled to
//! the display: the hints along the foot, the keyboard or the PIN pad, the
//! panel about controllers and the power menu. In Night, as the login
//! screen always is.
//!
//! It covers the display only while something of the controller's shows
//! (`Couch::showing`), so the mouse and keyboard meet the login screen
//! itself otherwise.

use aizome_gamepad::{Button, Family};
use aizome_kit::Icon;
use aizome_kit::layout::{Point, Rect};
use aizome_kit::sidestep_ui::Canvas;
use aizome_kit::theme::{Role, Theme};
use aizome_sofa::keyboard::Keyboard;
use aizome_sofa::look::*;
use aizome_sofa::{Fit, H, W, pinpad};

use crate::pads::POWER;

/// The design's foot, where the hints are.
const FOOT_Y: f64 = H - 64.0;
const MARGIN: f64 = 96.0;

/// The panel about controllers.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Help {
    pub family: Family,
    pub ok: Button,
    pub back: Button,
    /// The controller in hand has a Home button (else Select and Start
    /// held stand for it).
    pub guide: bool,
    /// The controllers there are: name, battery.
    pub pads: Vec<(String, Option<(u8, bool)>)>,
    /// There's a TV account to say how it signs in.
    pub tv: bool,
}

/// What's drawn.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Scene {
    pub hints: Vec<(Button, String)>,
    pub family: Family,
    pub keys: Option<Keyboard>,
    pub pin: Option<pinpad::Pad>,
    pub help: Option<Help>,
    /// The power menu, its lit row.
    pub power: Option<usize>,
}

/// What a click hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A key of the keyboard ([`Keyboard::hit`]).
    Key(usize),
    /// The PIN pad's ([`pinpad::hit`]).
    Pin(usize),
    /// A hint along the foot: its button.
    Hint(Button),
    Close,
    Power(usize),
    /// Inside a panel, on nothing.
    Panel,
    /// Beside every panel.
    Outside,
}

const HELP_W: f64 = 1280.0;
const HELP_H: f64 = 800.0;
const STRIP_H: f64 = 84.0;

fn help_frame() -> Rect {
    let (x0, y0) = (((W - HELP_W) / 2.0).round(), ((H - HELP_H) / 2.0).round());
    Rect::new(x0, y0, x0 + HELP_W, y0 + HELP_H)
}

fn close_rect() -> Rect {
    let f = help_frame();
    Rect::new(f.center().x - 130.0, f.y1 - 44.0 - 76.0, f.center().x + 130.0, f.y1 - 44.0)
}

const ROW_H: f64 = 76.0;

fn power_frame() -> Rect {
    let w = 520.0;
    let x1 = W - MARGIN;
    let y0 = 112.0;
    Rect::new(x1 - w, y0, x1, y0 + STRIP_H + 16.0 + POWER.len() as f64 * ROW_H + 16.0)
}

fn power_row(i: usize) -> Rect {
    let f = power_frame();
    let y = f.y0 + STRIP_H + 16.0 + i as f64 * ROW_H;
    Rect::new(f.x0 + 16.0, y, f.x1 - 16.0, y + ROW_H - 8.0)
}

impl Scene {
    fn fit(b: Rect) -> Fit {
        Fit::new(b)
    }

    pub fn paint(&self, c: &mut Canvas, b: Rect, theme: &Theme) {
        let fit = Scene::fit(b);
        // A veil behind a panel, so it reads over the wallpaper.
        if self.help.is_some() || self.pin.is_some() {
            c.fill_rect(b, theme.surface().with_alpha(0.55));
        }
        c.save();
        fit.apply(c);
        let mut x = MARGIN;
        for (button, what) in &self.hints {
            x = hint(c, theme, self.family, *button, what, x, FOOT_Y);
        }
        if let Some(k) = &self.keys {
            k.paint(c, theme);
        }
        if let Some(p) = &self.pin {
            p.paint(c, theme);
        }
        if let Some(h) = &self.help {
            h.paint(c, theme);
        }
        if let Some(i) = self.power {
            paint_power(c, theme, i);
        }
        c.restore();
    }

    /// What's at window point `p` in a surface `size` big.
    pub fn hit(&self, b: Rect, p: Point) -> Target {
        let d = Scene::fit(b).to_design(p);
        if self.help.is_some() {
            return if close_rect().contains(d) {
                Target::Close
            } else if help_frame().contains(d) {
                Target::Panel
            } else {
                Target::Outside
            };
        }
        if self.power.is_some() {
            if let Some(i) = (0..POWER.len()).find(|i| power_row(*i).contains(d)) {
                return Target::Power(i);
            }
            return if power_frame().contains(d) { Target::Panel } else { Target::Outside };
        }
        if let Some(pad) = &self.pin {
            if let Some(i) = pinpad::hit(d, pad.done, pad.forgot) {
                return Target::Pin(i);
            }
            if pinpad::frame().contains(d) {
                return Target::Panel;
            }
        }
        if let Some(k) = &self.keys {
            if let Some(n) = k.hit(d) {
                return Target::Key(n);
            }
            if Keyboard::frame().contains(d) {
                return Target::Panel;
            }
        }
        if (d.y - FOOT_Y).abs() < 40.0 {
            let theme = Theme::night();
            let mut x = MARGIN;
            for (button, what) in &self.hints {
                let w = hint_width(&theme, self.family, *button, what);
                if d.x >= x - 20.0 && d.x < x + w + 20.0 {
                    return Target::Hint(*button);
                }
                x += w + HINT_GAP;
            }
        }
        Target::Outside
    }

    /// Whether `p` is over something the pointer may use (moving there
    /// keeps the controller's things up).
    pub fn over_panel(&self, b: Rect, p: Point) -> bool {
        !matches!(self.hit(b, p), Target::Outside)
    }
}

impl Help {
    fn paint(&self, c: &mut Canvas, theme: &Theme) {
        let f = help_frame();
        floating(c, theme, f);
        let strip = Rect::new(f.x0 + EDGE, f.y0 + EDGE, f.x1 - EDGE, f.y0 + STRIP_H);
        c.fill_rect(strip, theme.menu_title());
        icon(c, Icon::Gamepad2, Point::new(strip.x0 + 36.0 + 17.0, strip.center().y), 34.0, theme.focus());
        let spec = font(TITLE, SEMIBOLD);
        text_mid(
            c,
            theme,
            "Game controllers",
            strip.x0 + 36.0 + 34.0 + 18.0,
            strip.center().y,
            spec,
            theme.on_menu_title(),
        );
        let body = font(BODY - 2.0, REGULAR);
        let left = f.x0 + 64.0;
        let mut y = f.y0 + STRIP_H + 36.0;
        for line in wrap(
            theme,
            "Press a button on any controller to use it here. Until one is pressed, the login screen \
             is the keyboard's and the mouse's, and a key or the mouse makes it theirs again.",
            f.width() - 128.0,
            body,
        ) {
            text_mid(c, theme, &line, left, y + 18.0, body, theme.text());
            y += 40.0;
        }
        // Two columns: moving about, and typing.
        let col = (f.width() - 128.0) / 2.0;
        let top = y + 36.0;
        caption(c, theme, "Choosing", left, top, theme.text3());
        caption(c, theme, "Typing a password", left + col, top, theme.text3());
        let open =
            if self.guide { (Button::Guide, "This panel") } else { (Button::Select, "With Start, held: this panel") };
        let choosing = [
            (Button::Right, "Move: arrows or stick"),
            (self.ok, "Choose"),
            (self.back, "Back"),
            (Button::Start, "Log in"),
            open,
        ];
        let typing = [
            (self.ok, "Type the key"),
            (Button::West, "Delete"),
            (Button::North, "Space"),
            (Button::L2, "Capitals"),
            (Button::R2, "Symbols"),
        ];
        for (column, rows) in [(left, &choosing), (left + col, &typing)] {
            let mut ry = top + 52.0;
            for (b, what) in rows.iter() {
                let w = glyph_width(theme, self.family, *b, 22.0);
                button_glyph(c, theme, self.family, *b, Point::new(column + w / 2.0, ry), 22.0);
                text_mid(c, theme, what, column + w + 16.0, ry, font(SMALL + 2.0, REGULAR), theme.text2());
                ry += 56.0;
            }
        }
        let mut y = top + 52.0 + 5.0 * 56.0 + 8.0;
        if self.tv {
            let line = "TV opens Chanoma. Its PIN is pressed on the controller: each button in the legend is a digit.";
            let line = fit_text(theme, line, f.width() - 128.0, font(SMALL + 2.0, REGULAR));
            text_mid(c, theme, &line, left, y, font(SMALL + 2.0, REGULAR), theme.text2());
            y += 44.0;
        }
        let (status, ink) = match self.pads.as_slice() {
            [] => (
                "No controller is connected: pair one in Settings, under Bluetooth, or plug one in.".to_owned(),
                theme.text3(),
            ),
            pads => {
                let names: Vec<String> = pads
                    .iter()
                    .map(|(name, battery)| match battery {
                        Some((pct, _)) => format!("{name} ({pct}%)"),
                        None => name.clone(),
                    })
                    .collect();
                (format!("Connected: {}", names.join(", ")), theme.role(Role::Success))
            }
        };
        let status = fit_text(theme, &status, f.width() - 128.0, font(SMALL + 2.0, REGULAR));
        text_mid(c, theme, &status, left, y, font(SMALL + 2.0, REGULAR), ink);
        let r = close_rect();
        face(c, theme, r, theme.raised(), theme.line2());
        let w = hint_width(theme, self.family, self.back, "Close");
        hint(c, theme, self.family, self.back, "Close", r.center().x - w / 2.0, r.center().y);
    }
}

fn paint_power(c: &mut Canvas, theme: &Theme, at: usize) {
    let f = power_frame();
    floating(c, theme, f);
    let strip = Rect::new(f.x0 + EDGE, f.y0 + EDGE, f.x1 - EDGE, f.y0 + STRIP_H);
    c.fill_rect(strip, theme.menu_title());
    icon(c, Icon::Power, Point::new(strip.x0 + 32.0 + 15.0, strip.center().y), 30.0, theme.focus());
    text_mid(
        c,
        theme,
        "Power",
        strip.x0 + 32.0 + 30.0 + 18.0,
        strip.center().y,
        font(TITLE, SEMIBOLD),
        theme.on_menu_title(),
    );
    for (i, verb) in POWER.iter().enumerate() {
        let r = power_row(i);
        let ink = if i == at {
            lit(c, theme, r);
            ring(c, theme, r);
            theme.on_selection()
        } else {
            theme.text()
        };
        let glyph = match verb {
            crate::app::PowerVerb::Sleep => Icon::Moon,
            crate::app::PowerVerb::Restart => Icon::RotateCw,
            crate::app::PowerVerb::ShutDown => Icon::Power,
        };
        icon(c, glyph, Point::new(r.x0 + 40.0, r.center().y), 28.0, ink);
        text_mid(c, theme, verb.label(), r.x0 + 80.0, r.center().y, font(BODY, MEDIUM), ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panels_fit_the_design_and_clicks_find_them() {
        for r in [help_frame(), power_frame(), close_rect()] {
            assert!(r.x0 > 0.0 && r.x1 < W && r.y0 > 0.0 && r.y1 < H, "{r:?}");
        }
        let b = Rect::new(0.0, 0.0, 1440.0, 900.0);
        let fit = Fit::new(b);
        let scene = |help: bool, power: Option<usize>| Scene {
            hints: vec![(Button::South, "Choose".into())],
            family: Family::Xbox,
            keys: None,
            pin: None,
            help: help.then(|| Help {
                family: Family::Xbox,
                ok: Button::South,
                back: Button::East,
                guide: true,
                pads: Vec::new(),
                tv: false,
            }),
            power,
        };
        let s = scene(true, None);
        let at = |r: Rect| fit.to_window(r).center();
        assert_eq!(s.hit(b, at(close_rect())), Target::Close);
        assert_eq!(s.hit(b, at(help_frame())), Target::Panel);
        assert_eq!(s.hit(b, Point::new(2.0, 2.0)), Target::Outside);
        let s = scene(false, Some(0));
        assert_eq!(s.hit(b, at(power_row(2))), Target::Power(2));
        let s = scene(false, None);
        assert_eq!(
            s.hit(b, fit.to_window(Rect::new(MARGIN, FOOT_Y - 4.0, MARGIN + 8.0, FOOT_Y + 4.0)).center()),
            Target::Hint(Button::South)
        );
        assert!(!s.over_panel(b, Point::new(700.0, 300.0)));
    }
}
