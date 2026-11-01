//! What Chanoma draws: every screen is a 1920 × 1080 design scaled to fit
//! the television and centred ([`Fit`]), drawn on one canvas from a
//! [`Scene`] the program builds in `view`, in Aizome's look ([`look`]).
//!
//! Most screens wear the same header and foot. The header is the menu
//! bar's, without the bar: Aizome's mark and Chanoma's name at the left
//! (or the screen's title), and at the right what the systray would show
//! here: grown-up mode, each controller with its player and battery, the
//! time left today, and the clock as the menu bar sets it. The foot says
//! what the buttons do, drawn as the controller in hand prints them.
//!
//! **What redraws.** The scene's hash leaves out where the focus is; the
//! tiles and rows that move with it are the canvas's parts. Moving along a
//! row draws two tiles again, not the screen. Scrolling a row, opening a
//! menu or a minute turning over the clock draws it all.

pub mod home;
pub mod look;
pub mod menu;
pub mod picker;
pub mod pinpad;
pub mod screens;
pub mod settings;

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::rc::Rc;

use aizome_kit::Icon;
use aizome_kit::layout::{Point, Rect, Size};
use aizome_kit::sidestep_ui::{Canvas, Image};
use aizome_kit::theme::{Role, Theme};

use crate::pad::{Button, Family};
use look::*;

/// The design's size, in its own points.
pub const W: f64 = 1920.0;
pub const H: f64 = 1080.0;
/// Where screens start under the header.
pub const TOP: f64 = 176.0;
/// The side margin.
pub const MARGIN: f64 = 96.0;
/// The header's middle, and the foot's.
pub const HEAD_Y: f64 = 76.0;
pub const FOOT_Y: f64 = H - 64.0;

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
    /// Aizome's mark and Chanoma's name at the left (screens with a title
    /// of their own put it there instead).
    pub brand: bool,
    pub clock: String,
    /// "45 min left" and the like.
    pub left: Option<String>,
    /// Little time left: it turns amber.
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

impl Chrome {
    /// The hint at a design point: a click there presses its button.
    pub fn hint_at(&self, p: Point) -> Option<Button> {
        if (p.y - FOOT_Y).abs() > 40.0 {
            return None;
        }
        // The fonts are the same in every theme, so any measures them.
        let theme = Theme::night();
        let mut x = MARGIN;
        for (b, what) in &self.hints {
            let w = hint_width(&theme, self.family, *b, what);
            if p.x >= x - 20.0 && p.x < x + w + 20.0 {
                return Some(*b);
            }
            x += w + HINT_GAP;
        }
        None
    }
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
    /// The bar the pointer brings down over a page, a game or a film:
    /// the way home for a mouse.
    Top(TopBar),
}

/// A screen.
#[derive(Clone, Debug, PartialEq, Hash)]
pub enum View {
    Home(home::Home),
    Grid(home::Grid),
    Settings(settings::Page),
    /// Choosing a folder.
    Picker(picker::Picker),
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
        if !matches!(view, View::Home(_) | View::Grid(_) | View::Settings(_) | View::Picker(_)) {
            focus.hash(&mut h);
        }
        if let View::Settings(p) = &view {
            // The side list and the rows: which one has the focus decides
            // how both look.
            p.in_list(focus).hash(&mut h);
        }
        if let View::Picker(p) = &view {
            // The places scroll with the focus.
            p.side_first(focus).hash(&mut h);
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
            View::Picker(p) => p.parts(self.focus, &fit),
            _ => Vec::new(),
        }
    }

    /// What's at a window point.
    pub fn hit(&self, size: Size, p: Point) -> Option<Target> {
        let fit = Fit::of_size(size);
        let d = fit.to_design(p);
        // A click beside a menu or the pad puts it away.
        let outside = |frame: Rect| (!frame.contains(d)).then_some(Target::Outside);
        match &self.overlay {
            Some(Overlay::Pin(_)) => return pinpad::hit(d).map(Target::Key).or_else(|| outside(pinpad::frame())),
            Some(Overlay::Menu(m)) => return m.hit(d).map(Target::Item).or_else(|| outside(m.frame())),
            Some(Overlay::Top(_)) => {
                let top = fit.visible(Rect::from_origin_size(Point::ZERO, size)).y0;
                if d.y < top + TOP_BAR_H {
                    return top_home(top).contains(d).then_some(Target::Hint(Button::Guide));
                }
            }
            _ => {}
        }
        if let Some(b) = self.chrome.as_ref().and_then(|c| c.hint_at(d)) {
            return Some(Target::Hint(b));
        }
        match &self.view {
            View::Home(h) => h.hit(self.focus, d).map(|(row, col)| Target::Tile(row, col)),
            View::Grid(g) => g.hit(self.focus, d).map(Target::Item),
            View::Settings(p) => p.hit(self.focus, d),
            View::Picker(p) => p.hit(self.focus, d),
            View::Message(m) => m.hit(d).map(Target::Item),
            View::Clear => Some(Target::Picture),
        }
    }

    pub fn paint(&self, c: &mut Canvas, b: Rect, theme: &Theme) {
        let fit = Fit::new(b);
        let clear = matches!(self.view, View::Clear);
        if !clear {
            backdrop(c, theme, b);
        }
        c.saved(|c| {
            fit.apply(c);
            match &self.view {
                View::Home(h) => h.paint(c, theme, self.focus, &self.art),
                View::Grid(g) => g.paint(c, theme, self.focus, &self.art),
                View::Settings(p) => p.paint(c, theme, self.focus),
                View::Picker(p) => p.paint(c, theme, self.focus),
                View::Message(m) => m.paint(c, theme, self.focus),
                View::Clear => {}
            }
            if let Some(ch) = &self.chrome {
                paint_chrome(c, theme, ch);
            }
        });
        match &self.overlay {
            Some(o @ (Overlay::Pin(_) | Overlay::Menu(_))) => {
                // What's under it stays in sight, quietened.
                c.fill_rect(b, theme.surface().with_alpha(0.82));
                c.saved(|c| {
                    fit.apply(c);
                    match o {
                        Overlay::Pin(p) => p.paint(c, theme, Focus { row: self.modal_focus, col: 0 }),
                        Overlay::Menu(m) => {
                            let (family, ok) =
                                self.chrome.as_ref().map_or((Family::Generic, Button::South), |c| (c.family, c.ok));
                            m.paint(c, theme, Focus { row: self.modal_focus, col: 0 }, family, ok)
                        }
                        Overlay::Toast(..) | Overlay::Bar(..) | Overlay::Top(_) => {}
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
            Some(Overlay::Top(t)) => c.saved(|c| {
                let v = fit.visible(b);
                fit.apply(c);
                top_bar(c, theme, t, v);
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
    /// A hint at the foot, as pressing its button would; the bar's Home
    /// is [`Button::Guide`]'s.
    Hint(Button),
    /// Beside a menu or the PIN pad.
    Outside,
    /// A game's or a film's picture (or a page's, under the bar).
    Picture,
}

/// A film's bar along the foot: its name and how far through, on a
/// floating panel.
pub fn film_bar(c: &mut Canvas, theme: &Theme, label: &str, progress: u16) {
    let r = Rect::new(MARGIN, H - 200.0, W - MARGIN, H - 64.0);
    floating(c, theme, r);
    let spec = font(TITLE, SEMIBOLD);
    let l = fit_text(theme, label, r.width() - 80.0, spec);
    text_mid(c, theme, &l, r.x0 + 40.0, r.y0 + 46.0, spec, theme.text());
    let track = Rect::new(r.x0 + 40.0, r.y1 - 48.0, r.x1 - 40.0, r.y1 - 36.0);
    well(c, theme, track);
    let inner = track.inset(-EDGE);
    let done =
        Rect::new(inner.x0, inner.y0, inner.x0 + inner.width() * f64::from(progress.min(1000)) / 1000.0, inner.y1);
    if done.width() >= 1.0 {
        c.fill_rect(done, theme.selection());
    }
}

/// The bar the pointer brings down: Home, what shows, and the clock.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TopBar {
    pub title: String,
    pub clock: String,
}

/// The bar's height.
pub const TOP_BAR_H: f64 = 152.0;

/// Whether a window point is in the bar's band along the top.
pub fn in_top_bar(size: Size, p: Point) -> bool {
    let fit = Fit::of_size(size);
    fit.to_design(p).y < fit.visible(Rect::from_origin_size(Point::ZERO, size)).y0 + TOP_BAR_H
}

/// The bar's Home button, the bar's top at `top`.
pub fn top_home(top: f64) -> Rect {
    Rect::new(MARGIN, top + 36.0, MARGIN + 240.0, top + 116.0)
}

/// The bar along the top of `v` (the window, in the design's points): a
/// floating panel, Home lit at its left as a default button is.
pub fn top_bar(c: &mut Canvas, theme: &Theme, t: &TopBar, v: Rect) {
    floating(c, theme, Rect::new(v.x0, v.y0, v.x1, v.y0 + TOP_BAR_H));
    let home = top_home(v.y0);
    let y = home.center().y;
    lit(c, theme, home);
    icon(c, Icon::House, Point::new(home.x0 + 52.0, y), 40.0, theme.on_selection());
    text_mid(c, theme, "Home", home.x0 + 92.0, y, font(TITLE, SEMIBOLD), theme.on_selection());
    let mut right = W - MARGIN;
    if !t.clock.is_empty() {
        right -= text_right(c, theme, &t.clock, right, y, mono(TITLE, MEDIUM), theme.text()) + 48.0;
    }
    let spec = font(TITLE, SEMIBOLD);
    let title = fit_text(theme, &t.title, right - home.x1 - 48.0, spec);
    text_mid(c, theme, &title, home.x1 + 48.0, y, spec, theme.text2());
}

/// What a note is about, for its icon and rail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Note {
    #[default]
    Time,
    GrownUp,
    Kids,
    Controller,
    /// The way back.
    Home,
}

/// A note at the top, over whatever shows: a floating panel with a rail
/// in its kind's colour, as Aizome's notifications are.
pub fn toast(c: &mut Canvas, theme: &Theme, t: &str, note: Note) {
    let spec = font(BODY, MEDIUM);
    let size = measure(theme, t, spec);
    let (glyph, color) = match note {
        Note::Time => (Icon::Hourglass, theme.role(Role::Warning)),
        Note::GrownUp => (Icon::LockOpen, theme.focus()),
        Note::Kids => (Icon::Lock, theme.role(Role::Success)),
        Note::Controller => (Icon::Gamepad2, theme.text2()),
        Note::Home => (Icon::House, theme.text2()),
    };
    let w = (RAIL + 32.0 + 36.0 + 20.0 + size.width + 40.0).round();
    let r = Rect::new((W / 2.0 - w / 2.0).round(), 36.0, (W / 2.0 + w / 2.0).round(), 116.0);
    floating(c, theme, r);
    c.fill_rect(Rect::new(r.x0, r.y0, r.x0 + RAIL, r.y1), color);
    icon(c, glyph, Point::new(r.x0 + RAIL + 32.0 + 18.0, r.center().y), 36.0, color);
    text_mid(c, theme, t, r.x0 + RAIL + 32.0 + 36.0 + 20.0, r.center().y, spec, theme.text());
}

/// The header (Chanoma's name; grown-up mode, the controllers, the time
/// left and the clock) and the hints along the foot.
pub fn paint_chrome(c: &mut Canvas, theme: &Theme, ch: &Chrome) {
    if ch.brand {
        mark(c, theme, Point::new(MARGIN, HEAD_Y - 18.0), 9.0);
        text_mid(c, theme, "Chanoma", MARGIN + 54.0, HEAD_Y, font(TITLE, BOLD), theme.text());
    }
    let mut right = W - MARGIN;
    if !ch.clock.is_empty() {
        right -= text_right(c, theme, &ch.clock, right, HEAD_Y, mono(TITLE, MEDIUM), theme.text()) + 48.0;
    }
    if let Some(left) = &ch.left {
        let color = if ch.short { theme.role(Role::Warning) } else { theme.text2() };
        let w = text_right(c, theme, left, right, HEAD_Y, font(SMALL + 2.0, MEDIUM), color);
        icon(c, Icon::Hourglass, Point::new(right - w - 26.0, HEAD_Y), 30.0, color);
        right -= w + 44.0 + 48.0;
    }
    for p in ch.pads.iter().rev() {
        if let Some((pct, charging)) = p.battery {
            let (glyph, color) = battery(theme, pct, charging);
            icon(c, glyph, Point::new(right - 16.0, HEAD_Y), 32.0, color);
            right -= 32.0 + 10.0;
        }
        right -=
            text_right(c, theme, &format!("{}", p.slot + 1), right, HEAD_Y, mono(SMALL, MEDIUM), theme.text2()) + 8.0;
        icon(c, Icon::Gamepad2, Point::new(right - 18.0, HEAD_Y), 36.0, theme.text2());
        right -= 36.0 + 32.0;
    }
    if ch.grownup {
        chip(c, theme, "Grown-up", right - 16.0, HEAD_Y, 18.0);
    }
    if let Some(b) = &ch.banner {
        let w = text_right(c, theme, b, W - MARGIN, FOOT_Y, font(SMALL + 2.0, MEDIUM), theme.text());
        icon(c, Icon::Gamepad2, Point::new(W - MARGIN - w - 30.0, FOOT_Y), 36.0, theme.role(Role::Warning));
    }
    let mut x = MARGIN;
    for (b, what) in &ch.hints {
        x = hint(c, theme, ch.family, *b, what, x, FOOT_Y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(overlay: Option<Overlay>, hints: Vec<(Button, String)>) -> Scene {
        let chrome = Chrome { hints, family: Family::Xbox, ..Chrome::default() };
        Scene::new(View::Clear, Some(chrome), overlay, Focus::default(), 0, Rc::new(HashMap::new()), 0)
    }

    #[test]
    fn a_click_finds_the_way_home_whatever_shows() {
        let size = Size::new(W, H);
        // The hints along the foot are buttons.
        let s = scene(None, vec![(Button::South, "Choose".into()), (Button::East, "Back".into())]);
        assert_eq!(s.hit(size, Point::new(MARGIN + 10.0, FOOT_Y)), Some(Target::Hint(Button::South)));
        let back = MARGIN + hint_width(&Theme::night(), Family::Xbox, Button::South, "Choose") + HINT_GAP + 10.0;
        assert_eq!(s.hit(size, Point::new(back, FOOT_Y)), Some(Target::Hint(Button::East)));
        assert_eq!(s.hit(size, Point::new(back, FOOT_Y - 120.0)), Some(Target::Picture), "over a picture");
        // The bar's Home, at the window's top however the design sits in it.
        let top = TopBar { title: "Netflix".into(), clock: "17:00".into() };
        let bar = scene(Some(Overlay::Top(top)), Vec::new());
        assert_eq!(bar.hit(size, Point::new(200.0, 76.0)), Some(Target::Hint(Button::Guide)));
        assert_eq!(bar.hit(size, Point::new(1200.0, 76.0)), None, "the rest of the bar");
        assert_eq!(bar.hit(size, Point::new(1200.0, 600.0)), Some(Target::Picture), "under it");
        // A 16:10 window: the design sits 60 points down, the bar doesn't.
        let tall = Size::new(1920.0, 1200.0);
        assert_eq!(bar.hit(tall, Point::new(200.0, 76.0)), Some(Target::Hint(Button::Guide)));
        assert!(in_top_bar(tall, Point::new(500.0, 140.0)) && !in_top_bar(tall, Point::new(500.0, 400.0)));
        // Beside a menu puts it away; on it, not.
        let menu = menu::Menu::new("Netflix", &["Keep watching", "Go home"]);
        let frame = menu.frame();
        let m = scene(Some(Overlay::Menu(menu)), Vec::new());
        assert_eq!(m.hit(size, Point::new(40.0, 540.0)), Some(Target::Outside));
        assert_ne!(m.hit(size, Point::new(frame.center().x, frame.y0 + 10.0)), Some(Target::Outside));
    }
}
