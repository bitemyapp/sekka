//! What setup draws, in the login screen's look: the wallpaper under the
//! kachi scrim, Archivo on Night's colours, sunk fields with the focus
//! rail, and the mark at the foot, with a dot for each page of choices
//! above it. Each page is a column 440pt wide down the middle: a heading,
//! a line on what it's for, its controls, and Back and Continue. Every
//! control is reached with Tab, Return goes on, Escape goes back.

use std::rc::Rc;

use aizome_face::look::{self, Hue};
use aizome_face::widgets::drawn::drawn;
use aizome_face::widgets::field::field;
use aizome_face::widgets::keys::{focus_keys, keys};
use aizome_kit::element::{Element, MsgFn};
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Align, Constraints, Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, KeyEvent, NamedKey, PointerButton};
use aizome_kit::theme::color;
use aizome_kit::widget::{
    Event, EventCx, FocusRing, Focusable, LayoutCx, PaintCx, SemanticRole, Semantics, Status, Widget,
};
use aizome_kit::{Icon, Image};
use aizome_theme::{NIGHT, Rgba, typeface};

use crate::app::{Greeter, Msg};
use crate::setup::{Field, Input, Page, STEPS, Setup, SetupMsg, ZONE_ROWS};

/// The column the pages are set in.
pub const COL: f64 = 480.0;
/// A form's labels, and its controls beside them.
const LABEL_W: f64 = 132.0;
const CTRL_W: f64 = COL - LABEL_W;
const CTRL_H: f64 = 34.0;
const HINT_H: f64 = 26.0;
/// A hint that may take two lines (Genkan's words for a broken rule).
const HINT2_H: f64 = 40.0;
const BUTTON_H: f64 = 34.0;

fn m(s: SetupMsg) -> Msg {
    Msg::Setup(s)
}

/// The page on the display with the login, and its height.
pub fn page(g: &Greeter, s: &Setup, size: Size) -> (Element<Msg>, f64) {
    let (parts, h): (Vec<Element<Msg>>, f64) = match s.page {
        Page::Connecting => (vec![line_el("Getting ready…", 15.0, NIGHT.text2, 30.0)], 30.0),
        Page::Welcome => welcome(g, s),
        Page::Region => region(s),
        Page::Account => account(s),
        Page::Desktop => desktops(s),
        Page::Review => review(s),
        Page::Working => working(s),
        Page::Failed => failed(s),
        Page::Unavailable => unavailable(s),
    };
    let power = g.cfg.power;
    // Once the account exists, a failed setup can only be finished.
    let made = s.page == Page::Failed && s.failure.as_ref().is_some_and(|f| f.account_exists);
    let back = s.page.back() != s.page && !made;
    let body = focus_keys(aizome_kit::column(parts), move |k| match named(k) {
        Some(NamedKey::Escape) if back => Some(m(SetupMsg::Back)),
        Some(NamedKey::F(3)) if power => Some(Msg::PowerMenu),
        _ => None,
    });
    let left = ((size.width - COL) / 2.0).max(0.0).round();
    (aizome_kit::container(Element::from(body).id("setup-page")).padding([0.0, 0.0, 0.0, left]).into(), h)
}

fn named(k: &KeyEvent) -> Option<NamedKey> {
    match &k.key {
        Key::Named(n) => Some(*n),
        _ => None,
    }
}

// Type and pieces.

fn spec(size: f64, weight: f32, ink: Rgba) -> aizome_kit::text::TextSpec {
    look::ui(size, weight, color(ink))
}

/// One line of text across the column, centred.
fn line_el(text: &str, size: f64, ink: Rgba, h: f64) -> Element<Msg> {
    let t = text.to_owned();
    drawn(COL, h, (t.clone(), size.to_bits(), ink), move |cx| {
        let b = cx.bounds();
        cx.draw_text_in(&t, &spec(size, typeface::WEIGHT_REGULAR, ink), b, Align::Center);
    })
    .text(text, SemanticRole::Label)
    .into()
}

/// Text wrapped across the column (centred, or from the left), in up to
/// `lines` lines of `size`.
fn para(text: &str, size: f64, ink: Rgba, lines: usize, center: bool) -> (Element<Msg>, f64) {
    let lh = (size * 1.45).round();
    let h = lh * lines as f64;
    let t = text.to_owned();
    let el = drawn(COL, h, (t.clone(), size.to_bits(), ink, center), move |cx| {
        let mut sp = spec(size, typeface::WEIGHT_REGULAR, ink);
        sp.wrap = aizome_kit::sidestep_ui::Wrap::Words;
        sp.max_lines = lines;
        sp.line_height = 1.45;
        sp.align =
            if center { aizome_kit::sidestep_ui::Alignment::Center } else { aizome_kit::sidestep_ui::Alignment::Start };
        let layout = cx.layout_text(&t, &sp, Some(COL));
        cx.canvas.draw_text(&layout, Point::new(0.0, 0.0));
    })
    .text(text, SemanticRole::Label);
    (el.into(), h)
}

/// The page's heading and what it's for.
fn heading(title: &str, about: &str, about_lines: usize) -> (Vec<Element<Msg>>, f64) {
    let t = title.to_owned();
    let title_el = drawn(COL, 44.0, t.clone(), move |cx| {
        let b = cx.bounds();
        cx.draw_text_in(&t, &spec(26.0, typeface::WEIGHT_SEMIBOLD, NIGHT.text), b, Align::Center);
    })
    .text(title, SemanticRole::Label);
    let (about_el, h) = para(about, 14.0, NIGHT.text2, about_lines, true);
    (vec![title_el.into(), gap(6.0), about_el, gap(26.0)], 44.0 + 6.0 + h + 26.0)
}

fn gap(h: f64) -> Element<Msg> {
    drawn(COL, h, (), |_| {}).into()
}

/// A form row: the label, right-aligned against the control.
fn labelled(label: &str, control: Element<Msg>) -> Element<Msg> {
    let l = label.to_owned();
    let label_el = drawn(LABEL_W, CTRL_H, l.clone(), move |cx| {
        let b = cx.bounds();
        cx.draw_text_in(
            &l,
            &spec(13.0, typeface::WEIGHT_SEMIBOLD, NIGHT.text2),
            Rect::new(0.0, 0.0, b.x1 - 16.0, b.y1),
            Align::End,
        );
    })
    .text(label, SemanticRole::Label);
    aizome_kit::row(vec![label_el.into(), aizome_kit::container(control).width(CTRL_W).into()]).into()
}

/// The line under a control: how to fill it in, or what's wrong.
fn hint(text: &str, problem: bool) -> Element<Msg> {
    hint_lines(text, problem, 1)
}

/// The same in up to `lines` lines, wrapped under the control.
fn hint_lines(text: &str, problem: bool, lines: usize) -> Element<Msg> {
    let t = text.to_owned();
    let h = if lines > 1 { HINT2_H } else { HINT_H };
    let el = drawn(COL, h, (t.clone(), problem, lines), move |cx| {
        let night = aizome_kit::Theme::night();
        let ink = if problem { Hue::Red.color(&night) } else { color(NIGHT.text3) };
        let mut sp = look::ui(12.5, typeface::WEIGHT_REGULAR, ink);
        sp.wrap = aizome_kit::sidestep_ui::Wrap::Words;
        sp.max_lines = lines;
        let layout = cx.layout_text(&t, &sp, Some(CTRL_W - 4.0));
        cx.canvas.draw_text(&layout, Point::new(LABEL_W, 5.0));
    })
    .text(text, SemanticRole::Label);
    el.into()
}

/// A text field in its box.
fn text_input(
    id: &'static str,
    value: &str,
    placeholder: &str,
    secure: bool,
    invalid: bool,
    on: fn(String) -> Msg,
    submit: Msg,
) -> Element<Msg> {
    let mut f = field(value, |_| look::ui(15.0, typeface::WEIGHT_REGULAR, color(NIGHT.text)))
        .placeholder(placeholder)
        .height(CTRL_H)
        .framed(invalid)
        .on_change(on)
        .on_submit(submit);
    if secure {
        f = f.secure();
    }
    Element::from(f).id(id)
}

// The control: a button, a pop-up, a tile.

/// What a control looks like now.
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub focused: bool,
    pub hovered: bool,
    pub pressed: bool,
}

type Painter = Rc<dyn Fn(&mut PaintCx, Look)>;
type KeyMap = Rc<dyn Fn(&KeyEvent) -> Option<Msg>>;

/// Something drawn that Tab reaches and Return, Space or a click works.
pub struct Control {
    w: ControlWidget,
    common: aizome_kit::element::Common,
}

struct ControlWidget {
    size: Size,
    hash: u64,
    paint: Painter,
    activate: Option<MsgFn>,
    keys: Option<KeyMap>,
    label: String,
    role: SemanticRole,
    default: bool,
}

pub fn control(
    width: f64,
    height: f64,
    hash: impl std::hash::Hash,
    paint: impl Fn(&mut PaintCx, Look) + 'static,
) -> Control {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    hash.hash(&mut h);
    Control {
        w: ControlWidget {
            size: Size::new(width, height),
            hash: std::hash::Hasher::finish(&h),
            paint: Rc::new(paint),
            activate: None,
            keys: None,
            label: String::new(),
            role: SemanticRole::Button,
            default: false,
        },
        common: aizome_kit::element::Common::default(),
    }
}

impl Control {
    pub fn on_activate(mut self, msg: Msg) -> Self {
        self.w.activate = Some(MsgFn::new(msg));
        self
    }

    /// Keys it answers besides Return and Space (a tile's arrows).
    pub fn keys(mut self, f: impl Fn(&KeyEvent) -> Option<Msg> + 'static) -> Self {
        self.w.keys = Some(Rc::new(f));
        self
    }

    pub fn text(mut self, label: impl Into<String>, role: SemanticRole) -> Self {
        self.w.label = label.into();
        self.w.role = role;
        self
    }

    /// Return works it when nothing focused takes Return.
    pub fn default_button(mut self) -> Self {
        self.w.default = true;
        self
    }
}

impl aizome_kit::Layoutable for Control {
    fn common_mut(&mut self) -> &mut aizome_kit::element::Common {
        &mut self.common
    }
}

impl From<Control> for Element<Msg> {
    fn from(c: Control) -> Element<Msg> {
        let mut e = Element::new(c.w, Vec::new());
        *aizome_kit::Layoutable::common_mut(&mut e) = c.common;
        e
    }
}

impl Widget for ControlWidget {
    fn name(&self) -> &'static str {
        "setup-control"
    }

    fn layout(&mut self, _cx: &mut LayoutCx, bc: Constraints) -> Size {
        bc.constrain(self.size)
    }

    fn paint(&self, cx: &mut PaintCx) {
        let look = Look { focused: cx.is_focused(), hovered: cx.is_hovered(), pressed: cx.is_pressed() };
        (self.paint)(cx, look);
    }

    fn hash(&self, h: &mut PaintHasher) {
        h.add(&self.hash);
    }

    fn hit(&self) -> bool {
        true
    }

    fn hover_paints(&self) -> bool {
        true
    }

    fn focusable(&self) -> Focusable {
        if self.activate.is_some() || self.keys.is_some() { Focusable::Tab } else { Focusable::No }
    }

    fn focus_ring(&self) -> FocusRing {
        FocusRing::Never
    }

    fn text(&self) -> Option<&str> {
        (!self.label.is_empty()).then_some(self.label.as_str())
    }

    fn semantics(&self) -> Option<Semantics> {
        Some(Semantics { role: self.role, label: Some(self.label.clone()), ..Default::default() })
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::PointerDown { button: PointerButton::Left, .. } => {
                cx.invalidate();
                Status::Handled
            }
            Event::PointerUp { pos, button: PointerButton::Left, .. } => {
                cx.invalidate();
                if cx.bounds().contains(*pos)
                    && let Some(msg) = &self.activate
                {
                    cx.emit(msg);
                }
                Status::Handled
            }
            Event::Key(k) if k.pressed => match self.keys.as_ref().and_then(|f| f(k)) {
                Some(msg) => {
                    cx.emit(&MsgFn::new(msg));
                    Status::Handled
                }
                None => Status::Ignored,
            },
            _ => Status::Ignored,
        }
    }

    fn activate(&mut self, cx: &mut EventCx) -> bool {
        match &self.activate {
            Some(msg) => {
                cx.emit(msg);
                cx.invalidate();
                true
            }
            None => false,
        }
    }

    fn is_default(&self) -> bool {
        self.default
    }
}

/// The focus rail under a control that has the focus.
fn rail(cx: &mut PaintCx, b: Rect) {
    cx.canvas.fill_rect(Rect::new(b.x0, b.y1 - 2.0, b.x1, b.y1), color(NIGHT.focus));
}

/// A button: the selection's indigo for the one that goes on, an edged
/// one for the others.
fn button(id: &'static str, label: &str, width: f64, primary: bool, msg: Msg) -> Element<Msg> {
    let l = label.to_owned();
    let c = control(width, BUTTON_H, (l.clone(), primary), move |cx, look| {
        let b = cx.bounds();
        let body = Rect::new(0.0, 0.0, b.x1, b.y1 - 4.0);
        let (fill, ink) = if primary {
            let base = Rgba::hex(0x3d59a1);
            let fill = if look.pressed {
                base.mix(Rgba::hex(0x16161e), 0.2)
            } else if look.hovered {
                base.mix(Rgba::hex(0xffffff), 0.08)
            } else {
                base
            };
            (Some(fill), NIGHT.on_selection)
        } else {
            // The scrim's kachi, deeper, so the label reads on any wallpaper.
            let fill = if look.pressed {
                Rgba::hexa(0x2a2e44, 0.95)
            } else if look.hovered {
                Rgba::hexa(0x2a2e44, 0.8)
            } else {
                Rgba::hexa(0x16161e, 0.6)
            };
            (Some(fill), NIGHT.text)
        };
        if let Some(f) = fill {
            cx.canvas.fill_rect(body, color(f));
        }
        aizome_kit::draw::border(cx.canvas, body, color(if primary { NIGHT.bevel_lo } else { NIGHT.line2 }), 1.0);
        cx.draw_text_in(&l, &spec(14.0, typeface::WEIGHT_SEMIBOLD, ink), body, Align::Center);
        if look.focused {
            rail(cx, b);
        }
    })
    .on_activate(msg)
    .text(label, SemanticRole::Button);
    let c = if primary { c.default_button() } else { c };
    Element::from(c).id(id)
}

/// Back on the left (where there's a page to go back to), the button that
/// goes on at the right.
fn buttons(back: Option<(&'static str, &str, Msg)>, go: (&'static str, &str, Msg)) -> Element<Msg> {
    let w = 136.0;
    let Some((id, label, msg)) = back else {
        // Alone, in the middle.
        let side = ((COL - w) / 2.0).round();
        return aizome_kit::container(button(go.0, go.1, w, true, go.2)).padding([0.0, 0.0, 0.0, side]).into();
    };
    aizome_kit::row(vec![
        button(id, label, w, false, msg),
        drawn(COL - 2.0 * w, BUTTON_H, (), |_| {}).into(),
        button(go.0, go.1, w, true, go.2),
    ])
    .into()
}

/// A pop-up button: the choice, and a chevron; Return, Space, ↓ or a
/// click open its menu.
fn popup(id: &'static str, value: &str, label: &str, msg: Msg) -> Element<Msg> {
    let v = value.to_owned();
    let open = msg.clone();
    let c = control(CTRL_W, CTRL_H, v.clone(), move |cx, look| {
        let b = cx.bounds();
        cx.canvas.fill_rect(b, color(if look.hovered { NIGHT.raised } else { NIGHT.sunk }));
        aizome_kit::draw::border(cx.canvas, b, color(NIGHT.line2), 1.0);
        if look.focused {
            cx.canvas.fill_rect(Rect::new(0.0, 0.0, 2.0, b.y1), color(NIGHT.focus));
        }
        cx.draw_text_in(
            &v,
            &spec(15.0, typeface::WEIGHT_REGULAR, NIGHT.text),
            Rect::new(12.0, 0.0, b.x1 - 34.0, b.y1),
            Align::Start,
        );
        Icon::ChevronDown.draw(cx.canvas, Point::new(b.x1 - 26.0, (b.y1 - 14.0) / 2.0), 14.0, color(NIGHT.text3));
    })
    .on_activate(msg)
    .keys(move |k| matches!(named(k), Some(NamedKey::ArrowDown)).then(|| open.clone()))
    .text(format!("{label}: {value}"), SemanticRole::Button);
    Element::from(c).id(id)
}

// The pages.

fn welcome(g: &Greeter, s: &Setup) -> (Vec<Element<Msg>>, f64) {
    let logo = g.setup_logo.clone();
    let mark = drawn(COL, 96.0, logo.as_ref().map(|i| i.pixel_size()), move |cx| {
        let b = cx.bounds();
        match &logo {
            Some(img) => {
                let sz = img.size();
                let x = ((b.width() - sz.width) / 2.0).round();
                cx.canvas.draw_image(img, Rect::new(x, 16.0, x + sz.width, 16.0 + sz.height));
            }
            None => {
                let night = aizome_kit::Theme::night();
                look::mark(cx.canvas, &night, Point::new(((b.width() - 48.0) / 2.0).round(), 24.0), 48.0);
            }
        }
    })
    .text("Sekka", SemanticRole::Image);
    let device = s.device();
    let (mut parts, h) = heading(
        &format!("Set up your {device}"),
        "Choose your language, keyboard and time zone, then make your account. It takes a few minutes and needs no network.",
        2,
    );
    parts.insert(0, mark.into());
    parts.push(buttons(None, ("setup-start", "Continue", m(SetupMsg::Continue))));
    (parts, 96.0 + h + BUTTON_H)
}

fn region(s: &Setup) -> (Vec<Element<Msg>>, f64) {
    let offer = s.offer();
    let (mut parts, mut h) = heading(
        "Language and region",
        "Set the keyboard first: your password is typed on it now and at every login.",
        2,
    );
    let language = Setup::label(&offer.locales, &s.draft.locale);
    let keyboard = Setup::label(&offer.keyboards, &s.draft.keyboard);
    parts.push(labelled("Language", popup("setup-language", &language, "Language", m(SetupMsg::LanguageMenu))));
    parts.push(hint("For menus, dates and numbers in your apps.", false));
    parts.push(labelled("Keyboard", popup("setup-keyboard", &keyboard, "Keyboard", m(SetupMsg::KeyboardMenu))));
    parts.push(gap(8.0));
    parts.push(labelled(
        "",
        text_input(
            "setup-try",
            &s.try_text,
            "Type here to try it",
            false,
            false,
            |t| m(SetupMsg::TryText(t)),
            m(SetupMsg::Continue),
        ),
    ));
    parts.push(hint("", false));
    h += (CTRL_H + HINT_H) * 2.0 + 8.0;
    // The zone: what it is, a field to search for another, and matches.
    let current = match s.zone() {
        Some(z) if z.country.is_empty() || z.city == z.country || z.id == "UTC" => z.city.clone(),
        Some(z) => format!("{}, {}", z.city, z.country),
        None => s.draft.time_zone.clone(),
    };
    let field = text_input(
        "setup-zone",
        &s.zone_query,
        &current,
        false,
        false,
        |q| m(SetupMsg::ZoneQuery(q)),
        m(SetupMsg::ZoneSubmit),
    );
    let field = keys(field, |k| match named(k) {
        Some(NamedKey::ArrowDown) => Some(m(SetupMsg::ZoneMove(1))),
        Some(NamedKey::ArrowUp) => Some(m(SetupMsg::ZoneMove(-1))),
        _ => None,
    });
    parts.push(labelled("Time zone", field));
    h += CTRL_H;
    let matches = s.zone_matches(ZONE_ROWS);
    if matches.is_empty() {
        let says = if s.zone_query.trim().is_empty() {
            format!("{} now. Type a city or a country to change it.", s.draft.time_zone)
        } else {
            "No time zone matches. Try a nearby city.".to_owned()
        };
        parts.push(hint(&says, false));
        h += HINT_H;
        parts.push(gap(ZONE_ROWS as f64 * 28.0 - HINT_H + 8.0));
    } else {
        parts.push(gap(4.0));
        let n = matches.len();
        let panel = drawn(CTRL_W, n as f64 * 28.0 + 2.0, n, |cx| {
            let b = cx.bounds();
            cx.canvas.fill_rect(b, color(NIGHT.sunk));
            aizome_kit::draw::border(cx.canvas, b, color(NIGHT.line2), 1.0);
        });
        let rows: Vec<Element<Msg>> = matches.iter().enumerate().map(|(i, z)| zone_row(z, i == s.zone_pick)).collect();
        let list =
            aizome_kit::stack(vec![panel.into(), aizome_kit::container(aizome_kit::column(rows)).padding(1.0).into()]);
        parts.push(aizome_kit::container(list).padding([0.0, 0.0, 0.0, LABEL_W]).into());
        parts.push(gap((ZONE_ROWS - n) as f64 * 28.0 - 2.0 + 4.0 + 8.0));
    }
    h += ZONE_ROWS as f64 * 28.0 + 8.0;
    parts.push(gap(16.0));
    parts.push(buttons(
        Some(("setup-back", "Back", m(SetupMsg::Back))),
        ("setup-continue", "Continue", m(SetupMsg::Continue)),
    ));
    (parts, h + 16.0 + BUTTON_H)
}

fn zone_row(z: &crate::setup::Zone, picked: bool) -> Element<Msg> {
    let (city, country, id) = (z.city.clone(), z.country.clone(), z.id.clone());
    let label = format!("{city}, {country}");
    let row = drawn(CTRL_W - 2.0, 28.0, (label.clone(), picked), move |cx| {
        let b = cx.bounds();
        if picked {
            cx.canvas.fill_rect(b, color(NIGHT.selection));
        } else if cx.is_hovered() {
            cx.canvas.fill_rect(b, color(Rgba::hexa(0xc0caf5, 0.08)));
        }
        let ink = if picked { NIGHT.on_selection } else { NIGHT.text };
        let w = cx.draw_text_in(
            &city,
            &spec(14.0, typeface::WEIGHT_MEDIUM, ink),
            Rect::new(12.0, 0.0, b.x1 - 12.0, b.y1),
            Align::Start,
        );
        let ink2 = if picked { NIGHT.on_selection } else { NIGHT.text3 };
        cx.draw_text_in(
            &format!("{country} · {id}"),
            &spec(12.5, typeface::WEIGHT_REGULAR, ink2),
            Rect::new(12.0 + w + 10.0, 0.0, b.x1 - 12.0, b.y1),
            Align::Start,
        );
    })
    .hoverable()
    .on_click(m(SetupMsg::Zone(z.id.clone())))
    .text(label, SemanticRole::Button);
    row.into()
}

fn account(s: &Setup) -> (Vec<Element<Msg>>, f64) {
    let device = s.device();
    let (mut parts, mut h) = heading(
        "Make your account",
        &format!(
            "It administers this {device}. Its password unlocks the login and lock screens, and confirms changes to the system."
        ),
        2,
    );
    let rows: [(Input, &str, &str, String, bool, String); 5] = [
        (
            Input::FullName,
            "Full name",
            "Ada Lovelace",
            s.draft.full_name.clone(),
            false,
            "Shown on the login screen.".into(),
        ),
        (
            Input::Username,
            "User name",
            "ada",
            s.draft.username.clone(),
            false,
            "Lowercase letters, digits, - and _; it names your home folder.".into(),
        ),
        (Input::Password, "Password", "At least 12 characters", s.password.clone(), true, password_hint(s)),
        (Input::Repeat, "Confirm", "The same password again", s.repeat.clone(), true, String::new()),
        (
            Input::Hostname,
            "Computer name",
            "chriss-mac",
            s.draft.hostname.clone(),
            false,
            format!("How this {device} shows on a network."),
        ),
    ];
    for (input, label, placeholder, value, secure, about) in rows {
        let problem = s.shown_problem(input);
        let on: fn(String) -> Msg = match input {
            Input::FullName => |v| m(SetupMsg::FullName(v)),
            Input::Username => |v| m(SetupMsg::Username(v)),
            Input::Password => |v| m(SetupMsg::Password(v)),
            Input::Repeat => |v| m(SetupMsg::Repeat(v)),
            Input::Hostname => |v| m(SetupMsg::Hostname(v)),
        };
        let field =
            text_input(input.id(), &value, placeholder, secure, problem.is_some(), on, m(SetupMsg::Submit(input)));
        parts.push(labelled(label, field));
        parts.push(hint_lines(problem.as_deref().unwrap_or(&about), problem.is_some(), 2));
        h += CTRL_H + HINT2_H;
    }
    parts.push(gap(14.0));
    parts.push(buttons(
        Some(("setup-back", "Back", m(SetupMsg::Back))),
        ("setup-continue", "Continue", m(SetupMsg::Continue)),
    ));
    (parts, h + 14.0 + BUTTON_H)
}

/// The password's rule, and how far along it is.
fn password_hint(s: &Setup) -> String {
    let n = s.password.chars().count();
    if s.password.is_empty() {
        "At least 12 characters. A few unrelated words make a good one.".into()
    } else if s.problem(Field::Password).is_none() && s.settled() {
        "Long enough.".into()
    } else if n < 12 {
        format!("At least 12 characters: {} more.", 12 - n)
    } else {
        "Checking…".into()
    }
}

fn desktops(s: &Setup) -> (Vec<Element<Msg>>, f64) {
    let offer = s.offer();
    let (mut parts, h) = heading(
        "Choose your desktop",
        "Both are installed. The login screen offers the other one whenever you like.",
        2,
    );
    let n = offer.desktops.len().max(1);
    let gapw = 16.0;
    let tile_w = ((COL - gapw * (n as f64 - 1.0)) / n as f64).floor();
    let tile_h = 168.0;
    let mut tiles: Vec<Element<Msg>> = Vec::new();
    for (i, d) in offer.desktops.iter().enumerate() {
        if i > 0 {
            tiles.push(drawn(gapw, tile_h, (), |_| {}).into());
        }
        let chosen = d.id == s.draft.desktop;
        let (label, about) = (d.label.clone(), d.description.clone());
        let tile = control(tile_w, tile_h, (label.clone(), chosen), move |cx, look| {
            let b = cx.bounds();
            let body = Rect::new(0.0, 0.0, b.x1, b.y1 - 6.0);
            cx.canvas.fill_rect(body, color(if chosen { NIGHT.raised } else { NIGHT.sunk }));
            aizome_kit::draw::border(cx.canvas, body, color(if chosen { NIGHT.line2 } else { NIGHT.line }), 1.0);
            if !chosen {
                cx.canvas.fill_rect(body, color(Rgba::hexa(0x16161e, if look.hovered { 0.2 } else { 0.4 })));
            }
            let ink = if chosen { NIGHT.text } else { NIGHT.text2 };
            cx.draw_text_in(
                &label,
                &spec(18.0, typeface::WEIGHT_SEMIBOLD, ink),
                Rect::new(16.0, 12.0, b.x1 - 16.0, 40.0),
                Align::Start,
            );
            if chosen {
                cx.draw_text_in(
                    "Chosen",
                    &spec(12.0, typeface::WEIGHT_MEDIUM, NIGHT.focus),
                    Rect::new(16.0, 12.0, b.x1 - 16.0, 40.0),
                    Align::End,
                );
            }
            let mut sp = spec(13.0, typeface::WEIGHT_REGULAR, if chosen { NIGHT.text2 } else { NIGHT.text3 });
            sp.wrap = aizome_kit::sidestep_ui::Wrap::Words;
            sp.max_lines = 5;
            sp.line_height = 1.4;
            let layout = cx.layout_text(&about, &sp, Some(b.x1 - 32.0));
            cx.canvas.draw_text(&layout, Point::new(16.0, 48.0));
            // The choice's rail, as under the person picked at login; the
            // focus's, amber, where the keyboard is.
            if chosen || look.focused {
                let ink = if look.focused { NIGHT.focus } else { NIGHT.selection };
                cx.canvas.fill_rect(Rect::new(0.0, b.y1 - 3.0, b.x1, b.y1), color(ink));
            }
        })
        // A click (or Return) on another chooses it; on the chosen one, goes on.
        .on_activate(m(if chosen { SetupMsg::Continue } else { SetupMsg::Desktop(i) }))
        .keys(move |k| match named(k) {
            Some(NamedKey::ArrowLeft) => Some(m(SetupMsg::DesktopStep(-1))),
            Some(NamedKey::ArrowRight) => Some(m(SetupMsg::DesktopStep(1))),
            _ => None,
        })
        .text(format!("{}{}", d.label, if chosen { ", chosen" } else { "" }), SemanticRole::Button);
        tiles.push(Element::from(tile).id(format!("setup-desktop-{}", d.id)).key(d.id.clone()));
    }
    parts.push(aizome_kit::row(tiles).into());
    parts.push(hint_center("← → choose, Return continues"));
    parts.push(gap(10.0));
    parts.push(buttons(
        Some(("setup-back", "Back", m(SetupMsg::Back))),
        ("setup-continue", "Continue", m(SetupMsg::Continue)),
    ));
    (parts, h + tile_h + HINT_H + 10.0 + BUTTON_H)
}

fn hint_center(text: &str) -> Element<Msg> {
    line_el(text, 12.5, NIGHT.text3, HINT_H)
}

fn review(s: &Setup) -> (Vec<Element<Msg>>, f64) {
    let offer = s.offer();
    let device = s.device();
    let (mut parts, h) = heading("Ready to set up", &format!("Check your choices, then set up your {device}."), 1);
    let zone = s
        .zone()
        .map(|z| if z.id == "UTC" { "UTC".to_owned() } else { format!("{}, {} ({})", z.city, z.country, z.id) });
    let desktop = offer.desktops.iter().find(|d| d.id == s.draft.desktop).map(|d| d.label.clone()).unwrap_or_default();
    let rows: Vec<(&str, String)> = vec![
        ("Name", if s.draft.full_name.is_empty() { "—".into() } else { s.draft.full_name.clone() }),
        ("User name", s.draft.username.clone()),
        ("Computer name", s.draft.hostname.clone()),
        ("Language", Setup::label(&offer.locales, &s.draft.locale)),
        ("Keyboard", Setup::label(&offer.keyboards, &s.draft.keyboard)),
        ("Time zone", zone.unwrap_or_else(|| s.draft.time_zone.clone())),
        ("Desktop", desktop),
    ];
    let row_h = 32.0;
    let n = rows.len();
    let table_h = row_h * n as f64 + 12.0;
    let text: Vec<String> = rows.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    let rows_owned: Vec<(String, String)> = rows.into_iter().map(|(k, v)| (k.to_owned(), v)).collect();
    let table = drawn(COL, table_h, rows_owned.clone(), move |cx| {
        let b = cx.bounds();
        cx.canvas.fill_rect(b, color(NIGHT.sunk));
        aizome_kit::draw::border(cx.canvas, b, color(NIGHT.line2), 1.0);
        for (i, (k, v)) in rows_owned.iter().enumerate() {
            let y = 6.0 + i as f64 * row_h;
            if i > 0 {
                cx.canvas.fill_rect(Rect::new(16.0, y, b.x1 - 16.0, y + 1.0), color(NIGHT.line));
            }
            cx.draw_text_in(
                k,
                &spec(13.0, typeface::WEIGHT_SEMIBOLD, NIGHT.text2),
                Rect::new(16.0, y, LABEL_W, y + row_h),
                Align::Start,
            );
            cx.draw_text_in(
                v,
                &spec(14.0, typeface::WEIGHT_REGULAR, NIGHT.text),
                Rect::new(LABEL_W, y, b.x1 - 16.0, y + row_h),
                Align::Start,
            );
        }
    })
    .text(text.join("\n"), SemanticRole::Label);
    parts.push(table.into());
    parts.push(hint_left("Your password is set. Your account can administer this computer."));
    parts.push(gap(10.0));
    parts
        .push(buttons(Some(("setup-back", "Back", m(SetupMsg::Back))), ("setup-set-up", "Set Up", m(SetupMsg::SetUp))));
    (parts, h + table_h + HINT_H + 10.0 + BUTTON_H)
}

fn hint_left(text: &str) -> Element<Msg> {
    let t = text.to_owned();
    drawn(COL, HINT_H, t.clone(), move |cx| {
        cx.draw_text_in(
            &t,
            &spec(12.5, typeface::WEIGHT_REGULAR, NIGHT.text3),
            Rect::new(0.0, 6.0, COL, 22.0),
            Align::Center,
        );
    })
    .text(text, SemanticRole::Label)
    .into()
}

fn working(s: &Setup) -> (Vec<Element<Msg>>, f64) {
    let device = s.device();
    let (mut parts, h) =
        heading(&format!("Setting up your {device}…"), "Keep it on. Checking the configuration can take a minute.", 1);
    let at =
        s.progress.as_ref().map(|p| STEPS.iter().position(|(id, _)| *id == p.step).unwrap_or(p.index)).unwrap_or(0);
    let started = s.progress.is_some();
    let count = STEPS.len();
    let bar = drawn(COL, 12.0, (at, started), move |cx| {
        let b = cx.bounds();
        let track = Rect::new(0.0, 4.0, b.x1, 8.0);
        cx.canvas.fill_rect(track, color(NIGHT.sunk));
        let k = if started { (at as f64 + 0.5) / count as f64 } else { 0.0 };
        cx.canvas.fill_rect(Rect::new(0.0, 4.0, (b.x1 * k).round(), 8.0), color(NIGHT.focus));
    });
    parts.push(bar.into());
    parts.push(gap(16.0));
    for (i, (_, label)) in STEPS.iter().enumerate() {
        let state = if !started || i > at {
            0
        } else if i == at {
            1
        } else {
            2
        };
        let l = (*label).to_owned();
        let row = drawn(COL, 30.0, (l.clone(), state), move |cx| {
            let b = cx.bounds();
            let x = (b.width() / 2.0 - 120.0).round();
            match state {
                2 => Icon::Check.draw(cx.canvas, Point::new(x, 7.0), 16.0, color(NIGHT.green)),
                1 => cx.canvas.fill_rect(Rect::new(x + 5.0, 12.0, x + 11.0, 18.0), color(NIGHT.focus)),
                _ => cx.canvas.fill_rect(Rect::new(x + 6.0, 14.0, x + 10.0, 16.0), color(NIGHT.faint)),
            }
            let ink = match state {
                2 => NIGHT.text2,
                1 => NIGHT.text,
                _ => NIGHT.text3,
            };
            let w = if state == 1 { typeface::WEIGHT_SEMIBOLD } else { typeface::WEIGHT_REGULAR };
            cx.draw_text_in(&l, &spec(14.0, w, ink), Rect::new(x + 26.0, 0.0, b.x1, b.y1), Align::Start);
        })
        .text(format!("{label}{}", ["", " (now)", " (done)"][state]), SemanticRole::Label);
        parts.push(row.into());
    }
    (parts, h + 12.0 + 16.0 + 30.0 * STEPS.len() as f64)
}

fn failed(s: &Setup) -> (Vec<Element<Msg>>, f64) {
    let f = s.failure.clone().unwrap_or_default();
    let (title, about) = if f.can_retry {
        ("Setup didn't finish", "Nothing is lost: trying again finishes what was begun.")
    } else {
        ("Those choices can't be used", "Go back and change what it says below.")
    };
    let (mut parts, h) = heading(title, about, 1);
    let (msg, mh) = para(&f.message, 14.0, NIGHT.red, 5, true);
    parts.push(msg);
    parts.push(gap(20.0));
    // Logging in only once the account has its password; until then, made,
    // it can only be finished.
    let back = if f.can_log_in {
        Some(("setup-log-in", "Log In", m(SetupMsg::LogIn)))
    } else if f.account_exists || !f.can_retry {
        None
    } else {
        Some(("setup-back", "Back", m(SetupMsg::Back)))
    };
    let go = if f.can_retry {
        ("setup-retry", "Try Again", m(SetupMsg::Retry))
    } else {
        ("setup-retry", "Back", m(SetupMsg::Retry))
    };
    parts.push(buttons(back, go));
    (parts, h + mh + 20.0 + BUTTON_H)
}

fn unavailable(s: &Setup) -> (Vec<Element<Msg>>, f64) {
    let why = s.unavailable.clone().unwrap_or_default();
    let (mut parts, h) = heading(
        "Setup isn't available",
        "The service that sets up this computer didn't answer. Try again; if it still doesn't, restart the computer.",
        2,
    );
    let (msg, mh) = para(&why, 12.5, NIGHT.text3, 3, true);
    parts.push(msg);
    parts.push(gap(20.0));
    parts.push(buttons(None, ("setup-retry", "Try Again", m(SetupMsg::Reconnect))));
    (parts, h + mh + 20.0 + BUTTON_H)
}

/// The pages of choices as dots above the mark: the one showing is the
/// focus's amber.
pub fn dots(s: &Setup, size: Size) -> Element<Msg> {
    let at = Page::STEPS.iter().position(|p| *p == s.page);
    let shown = matches!(s.page, Page::Region | Page::Account | Page::Desktop | Page::Review);
    let el = drawn(size.width, 12.0, (at, shown), move |cx| {
        if !shown {
            return;
        }
        let b = cx.bounds();
        let n = Page::STEPS.len() as f64;
        let x0 = ((b.width() - (n * 8.0 + (n - 1.0) * 10.0)) / 2.0).round();
        for i in 0..Page::STEPS.len() {
            let x = x0 + i as f64 * 18.0;
            let ink = if Some(i) == at { NIGHT.focus } else { NIGHT.line2 };
            cx.canvas.fill_rect(Rect::new(x, 2.0, x + 8.0, 10.0), color(ink));
        }
    });
    let label = at.map(|i| format!("Step {} of {}", i + 1, Page::STEPS.len())).unwrap_or_default();
    aizome_kit::container(el.text(label, SemanticRole::Label)).padding([size.height - 104.0, 0.0, 0.0, 0.0]).into()
}

/// The Sekka lockup (an SVG the configuration names), drawn `height`
/// points tall at `scale`.
pub fn logo(path: &std::path::Path, height: f64, scale: f64) -> Option<Image> {
    use resvg::{tiny_skia, usvg};
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > 1 << 20 {
        return None;
    }
    let data = std::fs::read(path).ok()?;
    let tree = usvg::Tree::from_data(&data, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let px_h = (height * scale).round().max(1.0) as u32;
    let k = px_h as f32 / size.height();
    let px_w = (size.width() * k).ceil().max(1.0) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(px_w, px_h)?;
    resvg::render(&tree, tiny_skia::Transform::from_scale(k, k), &mut pixmap.as_mut());
    let rgba: Vec<u8> = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Some(Image::from_rgba(px_w, px_h, &rgba)?.with_scale(1.0 / scale.max(0.25)))
}
