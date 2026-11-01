//! What Chanoma draws: every screen is a 1920 × 1080 design scaled to fit
//! the television and centred ([`Fit`]), drawn on one canvas from a
//! [`Scene`] the program builds in `view`.
//!
//! **What redraws.** The scene's hash leaves out where the focus is; the
//! tiles and rows that move with it are the canvas's parts. Moving along a
//! row draws two tiles again, not the screen. Scrolling a row, opening a
//! menu or a minute turning over the clock draws it all.

pub mod home;
pub mod look;
pub mod menu;
pub mod pinpad;
pub mod screens;
pub mod settings;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;

use aizome_kit::layout::{Point, Rect, Size};
use aizome_kit::sidestep_ui::{Canvas, Color, Image};
use aizome_kit::theme::Theme;

use crate::pad::{Button, Family};
use look::*;

/// The design's size, in its own points.
pub const W: f64 = 1920.0;
pub const H: f64 = 1080.0;
/// Where screens start under the header.
pub const TOP: f64 = 196.0;
/// The side margin.
pub const MARGIN: f64 = 96.0;

/// How the design sits in a window.
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

/// A controller in the header.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PadBadge {
    pub slot: usize,
    pub family: Family,
    pub battery: Option<(u8, bool)>,
}

/// The header and footer most screens wear.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Chrome {
    pub greeting: String,
    pub clock: String,
    /// "45 min left" and the like.
    pub left: Option<String>,
    /// Little time left: the pill turns amber.
    pub short: bool,
    pub grownup: bool,
    pub pads: Vec<PadBadge>,
    /// What the buttons do here.
    pub hints: Vec<(Button, String)>,
    /// Whose buttons the hints show.
    pub family: Family,
    /// The button that says yes.
    pub ok: Button,
    /// A line along the foot (no controller, a battery running out).
    pub banner: Option<String>,
}

/// What's over a screen.
#[derive(Clone, Debug, PartialEq, Hash)]
pub enum Overlay {
    Pin(pinpad::Pad),
    Menu(menu::Menu),
    /// A note that goes by itself ("5 minutes left").
    Toast(String, Note),
    /// A film's bar: what plays and how far through (thousandths).
    Bar(String, u16),
}

/// A screen.
#[derive(Clone, Debug, PartialEq, Hash)]
pub enum View {
    Home(home::Home),
    Grid(home::Grid),
    Settings(settings::Page),
    Message(screens::Message),
    /// Nothing but the overlay, over a picture (a game, a film, a page).
    Clear,
}

/// Where the controller is on the screen: left out of the scene's hash.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Focus {
    pub row: usize,
    pub col: usize,
}

/// What the canvas draws.
#[derive(Clone)]
pub struct Scene {
    pub view: View,
    pub chrome: Option<Chrome>,
    pub overlay: Option<Overlay>,
    pub focus: Focus,
    /// The focus in the overlay (a menu, the PIN pad).
    pub modal_focus: usize,
    pub art: Rc<HashMap<PathBuf, Image>>,
    /// Counts pictures arriving, for the hash.
    pub art_rev: u64,
    content: u64,
}

impl Scene {
    pub fn new(
        view: View,
        chrome: Option<Chrome>,
        overlay: Option<Overlay>,
        focus: Focus,
        modal_focus: usize,
        art: Rc<HashMap<PathBuf, Image>>,
        art_rev: u64,
    ) -> Scene {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        view.hash(&mut h);
        chrome.hash(&mut h);
        overlay.hash(&mut h);
        art_rev.hash(&mut h);
        if overlay.is_some() {
            modal_focus.hash(&mut h);
        }
        // Screens whose focus isn't drawn as parts redraw with it.
        if !matches!(view, View::Home(_) | View::Grid(_) | View::Settings(_)) {
            focus.hash(&mut h);
        }
        if let View::Settings(p) = &view {
            // The side list and the rows: which one has the focus decides
            // how both look.
            p.in_list(focus).hash(&mut h);
        }
        Scene { view, chrome, overlay, focus, modal_focus, art, art_rev, content: h.finish() }
    }

    /// The parts that move with the focus, in the window's points.
    pub fn parts(&self, size: Size) -> Vec<(Rect, u64)> {
        if self.overlay.is_some() {
            return Vec::new();
        }
        let fit = Fit::of_size(size);
        match &self.view {
            View::Home(h) => h.parts(self.focus, &fit),
            View::Grid(g) => g.parts(self.focus, &fit),
            View::Settings(p) => p.parts(self.focus, &fit),
            _ => Vec::new(),
        }
    }

    /// What's at a window point.
    pub fn hit(&self, size: Size, p: Point) -> Option<Target> {
        let d = Fit::of_size(size).to_design(p);
        match &self.overlay {
            Some(Overlay::Pin(_)) => return pinpad::hit(d).map(Target::Key),
            Some(Overlay::Menu(m)) => return m.hit(d).map(Target::Item),
            _ => {}
        }
        match &self.view {
            View::Home(h) => h.hit(self.focus, d).map(|(row, col)| Target::Tile(row, col)),
            View::Grid(g) => g.hit(self.focus, d).map(Target::Item),
            View::Settings(p) => p.hit(self.focus, d),
            View::Message(m) => m.hit(d).map(Target::Item),
            View::Clear => None,
        }
    }

    pub fn paint(&self, c: &mut Canvas, b: Rect, theme: &Theme) {
        let fit = Fit::new(b);
        let clear = matches!(self.view, View::Clear);
        if !clear {
            backdrop(c, b);
        }
        c.saved(|c| {
            fit.apply(c);
            match &self.view {
                View::Home(h) => h.paint(c, theme, self.focus, &self.art),
                View::Grid(g) => g.paint(c, theme, self.focus, &self.art),
                View::Settings(p) => p.paint(c, theme, self.focus),
                View::Message(m) => m.paint(c, theme, self.focus),
                View::Clear => {}
            }
            if let Some(ch) = &self.chrome {
                paint_chrome(c, theme, ch);
            }
        });
        match &self.overlay {
            Some(o @ (Overlay::Pin(_) | Overlay::Menu(_))) => {
                c.fill_rect(b, VEIL);
                c.saved(|c| {
                    fit.apply(c);
                    match o {
                        Overlay::Pin(p) => p.paint(c, theme, Focus { row: self.modal_focus, col: 0 }),
                        Overlay::Menu(m) => {
                            let (family, ok) =
                                self.chrome.as_ref().map_or((Family::Generic, Button::South), |c| (c.family, c.ok));
                            m.paint(c, theme, Focus { row: self.modal_focus, col: 0 }, family, ok)
                        }
                        Overlay::Toast(..) | Overlay::Bar(..) => {}
                    }
                });
            }
            Some(Overlay::Toast(t, note)) => c.saved(|c| {
                fit.apply(c);
                toast(c, theme, t, *note);
            }),
            Some(Overlay::Bar(label, progress)) => c.saved(|c| {
                fit.apply(c);
                film_bar(c, theme, label, *progress);
            }),
            None => {}
        }
    }
}

impl Hash for Scene {
    fn hash<H: Hasher>(&self, h: &mut H) {
        self.content.hash(h);
    }
}

/// What a click landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Tile(usize, usize),
    Item(usize),
    Section(usize),
    Key(usize),
}

/// A film's bar along the foot: its name, its time, how far through.
pub fn film_bar(c: &mut Canvas, theme: &Theme, label: &str, progress: u16) {
    let r = Rect::new(MARGIN, H - 190.0, W - MARGIN, H - 60.0);
    look::card(c, r, 30.0, Color::rgba(0.07, 0.08, 0.2, 0.88));
    let l = fit_text(theme, label, r.width() - 80.0, font(34.0, 700.0));
    text(c, theme, &l, Point::new(r.x0 + 40.0, r.y0 + 22.0), font(34.0, 700.0), INK);
    let bar = Rect::new(r.x0 + 40.0, r.y1 - 40.0, r.x1 - 40.0, r.y1 - 28.0);
    c.fill(&aizome_kit::sidestep_ui::kurbo::RoundedRect::from_rect(bar, 6.0), Color::rgba(1.0, 1.0, 1.0, 0.2));
    let done = Rect::new(bar.x0, bar.y0, bar.x0 + bar.width() * f64::from(progress.min(1000)) / 1000.0, bar.y1);
    if done.width() >= 1.0 {
        c.fill(&aizome_kit::sidestep_ui::kurbo::RoundedRect::from_rect(done, 6.0), FOCUS);
    }
}

/// What a toast is about, for its picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Note {
    #[default]
    Time,
    GrownUp,
    Kids,
    Controller,
}

/// A note at the top, over whatever shows.
pub fn toast(c: &mut Canvas, theme: &Theme, t: &str, note: Note) {
    let spec = font(34.0, 700.0);
    let size = measure(theme, t, spec);
    let r = Rect::new(W / 2.0 - size.width / 2.0 - 90.0, 48.0, W / 2.0 + size.width / 2.0 + 40.0, 132.0);
    look::card(c, r, 42.0, Color::hex(0x2a2f6e));
    let at = Point::new(r.x0 + 52.0, r.center().y);
    match note {
        Note::Time => hourglass(c, at, 40.0, WARN),
        Note::GrownUp => padlock(c, at, 38.0, FOCUS),
        Note::Kids => padlock(c, at, 38.0, GOOD),
        Note::Controller => controller(c, at, 52.0, INK, Color::hex(0x2a2f6e)),
    }
    text(c, theme, t, Point::new(r.x0 + 90.0, r.center().y - size.height / 2.0), spec, INK);
}

/// The header (clock, time left, controllers) and the hints along the
/// foot.
pub fn paint_chrome(c: &mut Canvas, theme: &Theme, ch: &Chrome) {
    if !ch.clock.is_empty() {
        text(c, theme, &ch.greeting, Point::new(MARGIN, 52.0), font(30.0, 600.0), INK_SOFT);
        text(c, theme, &ch.clock, Point::new(MARGIN, 88.0), font(60.0, 800.0), INK);
    }
    let mut right = W - MARGIN;
    for p in ch.pads.iter().rev() {
        let at = Point::new(right - 46.0, 100.0);
        controller(c, at, 76.0, INK_SOFT, Color::hex(0x252c62));
        text_centred(c, theme, &format!("{}", p.slot + 1), Point::new(at.x, at.y + 46.0), font(22.0, 800.0), INK_SOFT);
        if let Some((pct, charging)) = p.battery {
            battery(c, Point::new(at.x, at.y - 46.0), 34.0, f64::from(pct) / 100.0, charging);
        }
        right -= 104.0;
    }
    if let Some(left) = &ch.left {
        let r = pill(
            c,
            theme,
            left,
            right - 12.0,
            74.0,
            if ch.short { WARN } else { Color::hex(0x2c3474) },
            if ch.short { Color::hex(0x2a1e05) } else { INK },
        );
        hourglass(c, Point::new(r.x0 - 30.0, r.center().y), 38.0, if ch.short { WARN } else { INK_SOFT });
        right = r.x0 - 64.0;
    }
    if ch.grownup {
        let r = pill(c, theme, "Grown-up", right - 12.0, 74.0, FOCUS, Color::hex(0x2a1e05));
        let _ = r;
    }
    let y = H - 62.0;
    if let Some(b) = &ch.banner {
        let spec = font(30.0, 700.0);
        let size = measure(theme, b, spec);
        let r = Rect::new(MARGIN - 20.0, y - 112.0, MARGIN + size.width + 110.0, y - 44.0);
        c.fill(&aizome_kit::sidestep_ui::kurbo::RoundedRect::from_rect(r, 34.0), Color::hex(0x3b2a12));
        controller(c, Point::new(r.x0 + 52.0, r.center().y), 52.0, WARN, Color::hex(0x3b2a12));
        text(c, theme, b, Point::new(r.x0 + 96.0, r.center().y - size.height / 2.0), spec, WARN);
    }
    let mut x = MARGIN;
    for (b, what) in &ch.hints {
        x = hint(c, theme, ch.family, *b, what, x, y);
    }
}
