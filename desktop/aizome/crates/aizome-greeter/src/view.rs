//! What the login screen draws: the lock screen's look (the wallpaper under
//! a kachi scrim, the time in IBM Plex Mono and the date, the picture,
//! the name and the password field with its focus rail, the mark), with
//! what logging in adds: the computer's name and the power menu at the
//! top, the people to pick from, and the session under the field.

use aizome_face::look::{self, Hue};
use aizome_face::widgets::drawn::{around, drawn};
use aizome_face::widgets::field::field;
use aizome_face::widgets::keys::{focus_keys, keys};
use aizome_kit::element::Element;
use aizome_kit::layout::{Align, Point, Rect, Size};
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, KeyEvent, NamedKey};
use aizome_kit::theme::color;
use aizome_kit::widget::SemanticRole;
use aizome_kit::{Icon, Image};
use aizome_theme::{NIGHT, Rgba, typeface};

use crate::app::{Greeter, Msg, Stage};
use crate::login::{Step, Tone};

const FIELD_W: f64 = 260.0;
/// A person in the row: their picture and name.
const TILE_W: f64 = 112.0;
const PICTURE: f64 = 72.0;
const CLOCK_H: f64 = 120.0;
const WHO_H: f64 = 120.0;
const FIELD_H: f64 = 34.0;
const LINE_H: f64 = 30.0;

pub fn view(g: &Greeter, _window: WindowKey, output: Option<u32>) -> Element<Msg> {
    let Some(output) = output else { return aizome_kit::column(Vec::new()).into() };
    let size = g.frames.get(&output).copied().unwrap_or(Size::new(1440.0, 900.0));
    let main = Some(output) == g.main_output;
    let mut layers: Vec<Element<Msg>> = vec![ground(size, g.wallpaper(output))];
    let mut column: Vec<Element<Msg>> = vec![clock(size, &g.now.time(), &g.now.date())];
    let mut block_h = CLOCK_H;
    if main {
        let (content, h) = match g.stage {
            Stage::People => people(g, size),
            Stage::Password => password(g, size, false),
            Stage::Name if g.login.user.is_none() || g.login.step == Step::Nobody => name(g, size),
            Stage::Name => password(g, size, true),
        };
        column.push(content);
        block_h += h;
    }
    let top = ((size.height - block_h) / 2.0 - 40.0).max(56.0);
    layers.push(aizome_kit::column(column).padding([top, 0.0, 0.0, 0.0]).into());
    if main {
        layers.push(top_bar(g, size));
        layers.push(mark(size));
    }
    aizome_kit::stack(layers).width(size.width).height(size.height).into()
}

/// The wallpaper, covering the display, under the kachi scrim at 70%.
fn ground(size: Size, wall: Option<Image>) -> Element<Msg> {
    drawn(size.width, size.height, wall.as_ref().map(|i| i.pixel_size()), move |cx| {
        let b = cx.bounds();
        cx.canvas.fill_rect(b, color(Rgba::hex(0x16161e)));
        if let Some(img) = &wall {
            let s = Size::new(img.pixel_size().0 as f64, img.pixel_size().1 as f64);
            if s.width > 0.0 && s.height > 0.0 {
                let k = (b.width() / s.width).max(b.height() / s.height);
                let (cw, ch) = (b.width() / k, b.height() / k);
                let src =
                    Rect::new((s.width - cw) / 2.0, (s.height - ch) / 2.0, (s.width + cw) / 2.0, (s.height + ch) / 2.0);
                cx.canvas.draw_image_with(
                    img,
                    Some(src),
                    b,
                    aizome_face::wallpaper::options(img, b, cx.canvas.scale()),
                );
            }
        }
        cx.canvas.fill_rect(b, color(Rgba::hexa(0x16161e, 0.7)));
    })
    .into()
}

fn clock(size: Size, time: &str, date: &str) -> Element<Msg> {
    let (time, date) = (time.to_owned(), date.to_owned());
    let label = time.clone();
    drawn(size.width, CLOCK_H, (time.clone(), date.clone()), move |cx| {
        let b = cx.bounds();
        let ts = look::digits(64.0, typeface::WEIGHT_SEMIBOLD, color(NIGHT.text));
        cx.draw_text_in(&time, &ts, Rect::new(0.0, 0.0, b.x1, 86.0), Align::Center);
        let ds = look::ui(17.0, typeface::WEIGHT_REGULAR, color(NIGHT.text2));
        cx.draw_text_in(&date, &ds, Rect::new(0.0, 86.0, b.x1, 116.0), Align::Center);
    })
    .text(label, SemanticRole::Label)
    .into()
}

/// The computer's name at the top left; the power menu's button at the
/// top right.
fn top_bar(g: &Greeter, size: Size) -> Element<Msg> {
    let host = g.hostname.clone();
    let name = drawn(size.width / 2.0, 44.0, host.clone(), move |cx| {
        let s = look::ui(13.0, typeface::WEIGHT_SEMIBOLD, color(NIGHT.text2));
        cx.draw_text_in(&host, &s, Rect::new(24.0, 0.0, cx.bounds().x1, 44.0), Align::Start);
    })
    .text(g.hostname.clone(), SemanticRole::Label);
    let mut items: Vec<(f64, Element<Msg>)> = vec![(0.0, name.into())];
    if g.cfg.power {
        let button = drawn(32.0, 28.0, "power", |cx| {
            let b = cx.bounds();
            if cx.is_hovered() {
                cx.canvas.fill_rect(b, color(Rgba::hexa(0xc0caf5, 0.12)));
            }
            Icon::Power.draw(cx.canvas, Point::new(8.0, 6.0), 16.0, color(NIGHT.text2));
        })
        .hoverable()
        .on_click(Msg::PowerMenu)
        .text("Power", SemanticRole::Button);
        items.push((size.width - 24.0 - 32.0, Element::from(button).id("greeter-power")));
    }
    aizome_kit::stack(
        items
            .into_iter()
            .map(|(x, e)| aizome_kit::container(e).padding([if x == 0.0 { 0.0 } else { 8.0 }, 0.0, 0.0, x]).into())
            .collect(),
    )
    .width(size.width)
    .height(44.0)
    .into()
}

/// The mark, at the foot, as on the lock screen.
fn mark(size: Size) -> Element<Msg> {
    let m = drawn(size.width, 40.0, "mark", |cx| {
        let b = cx.bounds();
        let night = aizome_kit::Theme::night();
        look::mark(cx.canvas, &night, Point::new(((b.width() - 24.0) / 2.0).round(), 8.0), 24.0);
    });
    aizome_kit::container(m).padding([size.height - 72.0, 0.0, 0.0, 0.0]).into()
}

/// A picture, or the initial on the selection colour, 72pt square.
fn picture(cx: &mut aizome_kit::widget::PaintCx, tile: Rect, picture: Option<&Image>, initial: &str) {
    match picture {
        Some(p) => cx.canvas.draw_image(p, tile),
        None => {
            cx.canvas.fill_rect(tile, color(NIGHT.selection));
            let s = look::ui(34.0, typeface::WEIGHT_SEMIBOLD, color(NIGHT.on_selection));
            cx.draw_text_in(initial, &s, tile, Align::Center);
        }
    }
    aizome_kit::draw::border(cx.canvas, tile, color(NIGHT.bevel_lo), 1.0);
}

/// The people to pick from, in a row; the selection has the focus rail
/// under its picture. ← → choose, Return picks, a click picks.
fn people(g: &Greeter, size: Size) -> (Element<Msg>, f64) {
    let n = g.users.len().max(1) as f64;
    let left = ((size.width - n * TILE_W) / 2.0).max(0.0);
    let tiles: Vec<Element<Msg>> = g
        .users
        .iter()
        .enumerate()
        .map(|(i, u)| {
            let selected = i == g.selected;
            let pic = g.avatars.get(&u.name).cloned();
            let (initial, real) = (u.initial(), u.real_name.clone());
            drawn(TILE_W, WHO_H, (real.clone(), selected, pic.is_some()), move |cx| {
                let b = cx.bounds();
                let tile =
                    Rect::new(((b.width() - PICTURE) / 2.0).round(), 8.0, ((b.width() + PICTURE) / 2.0).round(), 80.0);
                picture(cx, tile, pic.as_ref(), &initial);
                if selected {
                    cx.canvas.fill_rect(Rect::new(tile.x0, tile.y1 + 4.0, tile.x1, tile.y1 + 6.0), color(NIGHT.focus));
                } else {
                    cx.canvas.fill_rect(tile, color(Rgba::hexa(0x16161e, 0.45)));
                }
                let (weight, ink) =
                    if selected { (typeface::WEIGHT_TAB, NIGHT.text) } else { (typeface::WEIGHT_REGULAR, NIGHT.text3) };
                let s = look::ui(14.0, weight, color(ink));
                cx.draw_text_in(&real, &s, Rect::new(2.0, 90.0, b.x1 - 2.0, 112.0), Align::Center);
            })
            .on_click(Msg::Pick(i))
            .text(u.real_name.clone(), SemanticRole::Button)
            .into()
        })
        .collect();
    let row = aizome_kit::container(aizome_kit::row(tiles)).padding([0.0, 0.0, 0.0, left]);
    let hint = drawn(size.width, LINE_H, "people-hint", |cx| {
        let s = look::ui(12.5, typeface::WEIGHT_REGULAR, color(NIGHT.text3));
        cx.draw_text_in("Choose with ← → and Return", &s, Rect::new(0.0, 8.0, cx.bounds().x1, 28.0), Align::Center);
    });
    let mut parts: Vec<Element<Msg>> = vec![row.into(), hint.into()];
    parts.push(message(g, size));
    let power = g.cfg.power;
    let body = focus_keys(aizome_kit::column(parts), move |k| match named(k) {
        Some(NamedKey::ArrowLeft) => Some(Msg::Previous),
        Some(NamedKey::ArrowRight) => Some(Msg::Next),
        Some(NamedKey::Enter) => Some(Msg::Pick(usize::MAX)),
        Some(NamedKey::F(3)) if power => Some(Msg::PowerMenu),
        _ => None,
    })
    .id("greeter-people");
    (body, WHO_H + LINE_H + LINE_H)
}

fn named(k: &KeyEvent) -> Option<NamedKey> {
    match &k.key {
        Key::Named(n) => Some(*n),
        _ => None,
    }
}

/// The person logging in, the field, their session, and what's to say.
fn password(g: &Greeter, size: Size, typed: bool) -> (Element<Msg>, f64) {
    let (real, initial, pic) = match (typed, g.user()) {
        (false, Some(u)) => (u.real_name.clone(), u.initial(), g.avatars.get(&u.name).cloned()),
        _ => {
            let n = g.login.user.clone().unwrap_or_default();
            let i = n.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
            (n, i, None)
        }
    };
    let who = drawn(size.width, WHO_H, (real.clone(), pic.is_some()), move |cx| {
        let b = cx.bounds();
        let tile = Rect::new(((b.width() - PICTURE) / 2.0).round(), 8.0, ((b.width() + PICTURE) / 2.0).round(), 80.0);
        picture(cx, tile, pic.as_ref(), &initial);
        let s = look::ui(16.0, typeface::WEIGHT_TAB, color(NIGHT.text));
        cx.draw_text_in(&real, &s, Rect::new(0.0, 88.0, b.x1, 112.0), Align::Center);
    })
    .text(g.user().map(|u| u.real_name.clone()).or(g.login.user.clone()).unwrap_or_default(), SemanticRole::Label);
    let session = g.session().map(|s| s.name.clone()).unwrap_or_default();
    let (secret, placeholder) = match &g.login.step {
        Step::Ask { secret, prompt } => (*secret, prompt.clone()),
        Step::Starting | Step::Done => (true, format!("Starting {session}…")),
        Step::Failed => (true, "Press Return to try again".to_owned()),
        Step::Waiting | Step::Nobody => (true, "Checking…".to_owned()),
    };
    let mut f = field(&g.password, |_| look::ui(15.0, typeface::WEIGHT_REGULAR, color(NIGHT.text)))
        .placeholder(placeholder)
        .height(32.0)
        .on_change(Msg::Password)
        .on_submit(Msg::Submit)
        .on_caps(Msg::Caps);
    if secret {
        f = f.secure();
    }
    let boxed = boxed_field(Element::from(f).id("greeter-password"), g.login.failed);
    let off = crate::app::shake_offset(g.shake);
    let field_row =
        aizome_kit::container(boxed).padding([0.0, 0.0, 0.0, ((size.width - FIELD_W) / 2.0 + off).max(0.0)]);
    let several = g.sessions.len() > 1;
    // As wide as the name (and its chevron), centred: the menu hangs from it.
    let label = session.clone();
    let chooser = drawn(0.0, LINE_H, (session.clone(), several), move |cx| {
        let b = cx.bounds();
        let s = look::ui(13.0, typeface::WEIGHT_REGULAR, color(NIGHT.text2));
        if several && cx.is_hovered() {
            cx.canvas.fill_rect(Rect::new(0.0, 4.0, b.x1, 30.0), color(Rgba::hexa(0xc0caf5, 0.10)));
        }
        let w = cx.draw_text_in(&session, &s, Rect::new(8.0, 6.0, b.x1, 28.0), Align::Start);
        if several {
            Icon::ChevronDown.draw(cx.canvas, Point::new(8.0 + w + 6.0, 11.0), 12.0, color(NIGHT.text3));
        }
    })
    .fit(move |cx| {
        let s = look::ui(13.0, typeface::WEIGHT_REGULAR, color(NIGHT.text2));
        let w = cx.measure(&label, &s).width;
        (8.0 + w + if several { 6.0 + 12.0 } else { 0.0 } + 8.0).ceil()
    })
    .hoverable()
    .text(g.session().map(|s| format!("Session: {}", s.name)).unwrap_or_default(), SemanticRole::Button);
    let chooser = if several { chooser.on_click(Msg::SessionMenu) } else { chooser };
    let chooser = aizome_kit::container(Element::from(chooser).id("greeter-session"))
        .width(size.width)
        .align_x(Align::Center)
        .padding([4.0, 0.0, 0.0, 0.0]);
    let mut parts: Vec<Element<Msg>> = vec![who.into(), field_row.into(), chooser.into(), message(g, size)];
    let mut h = WHO_H + FIELD_H + 4.0 + LINE_H + LINE_H;
    let others = g.cfg.user_list && g.users.len() > 1 && !typed;
    if others {
        let link = drawn(size.width, LINE_H, "others", |cx| {
            let s = look::ui(
                12.5,
                typeface::WEIGHT_REGULAR,
                color(if cx.is_hovered() { NIGHT.text2 } else { NIGHT.text3 }),
            );
            cx.draw_text_in("Log in as someone else", &s, Rect::new(0.0, 4.0, cx.bounds().x1, 24.0), Align::Center);
        })
        .hoverable()
        .on_click(Msg::People)
        .text("Log in as someone else", SemanticRole::Button);
        parts.push(link.into());
        h += LINE_H;
    }
    let power = g.cfg.power;
    let body = keys(aizome_kit::column(parts), move |k| match named(k) {
        Some(NamedKey::Escape) if others => Some(Msg::People),
        Some(NamedKey::F(2)) if several => Some(Msg::SessionMenu),
        Some(NamedKey::F(3)) if power => Some(Msg::PowerMenu),
        _ => None,
    });
    (body, h)
}

/// The user name to log in as (no list, or no one on it).
fn name(g: &Greeter, size: Size) -> (Element<Msg>, f64) {
    let who = drawn(size.width, WHO_H, "name", |cx| {
        let b = cx.bounds();
        let tile = Rect::new(((b.width() - PICTURE) / 2.0).round(), 8.0, ((b.width() + PICTURE) / 2.0).round(), 80.0);
        cx.canvas.fill_rect(tile, color(NIGHT.raised));
        Icon::UserRound.draw(cx.canvas, Point::new(tile.x0 + 20.0, tile.y0 + 20.0), 32.0, color(NIGHT.text2));
        aizome_kit::draw::border(cx.canvas, tile, color(NIGHT.bevel_lo), 1.0);
        let s = look::ui(16.0, typeface::WEIGHT_TAB, color(NIGHT.text));
        cx.draw_text_in("Log In", &s, Rect::new(0.0, 88.0, b.x1, 112.0), Align::Center);
    });
    let f = field(&g.name, |_| look::ui(15.0, typeface::WEIGHT_REGULAR, color(NIGHT.text)))
        .placeholder("User name")
        .height(32.0)
        .on_change(Msg::Name)
        .on_submit(Msg::SubmitName);
    let boxed = boxed_field(Element::from(f).id("greeter-name"), 0);
    let row = aizome_kit::container(boxed).padding([0.0, 0.0, 0.0, ((size.width - FIELD_W) / 2.0).max(0.0)]);
    let parts: Vec<Element<Msg>> = vec![who.into(), row.into(), message(g, size)];
    (aizome_kit::column(parts).into(), WHO_H + FIELD_H + LINE_H)
}

/// The field in its box: sunk, a hairline, and the focus rail at the left
/// (red once a try failed).
fn boxed_field(f: Element<Msg>, failed: u32) -> Element<Msg> {
    around(f, [0.0, 10.0, 0.0, 12.0], Size::new(FIELD_W, FIELD_H), ("greeter-field", failed > 0), move |cx| {
        let b = cx.bounds();
        cx.canvas.fill_rect(b, color(NIGHT.sunk));
        aizome_kit::draw::border(cx.canvas, b, color(NIGHT.line2), 1.0);
        cx.canvas.fill_rect(Rect::new(0.0, 0.0, 2.0, b.y1), color(NIGHT.focus));
    })
    .into()
}

/// What's to say under the field: why the last try failed, what PAM says,
/// Caps Lock, or why the power menu didn't do it.
fn message(g: &Greeter, size: Size) -> Element<Msg> {
    let msg = if let Some((m, tone)) = &g.login.message {
        Some((m.clone(), if *tone == Tone::Error { Hue::Red } else { Hue::Text2 }))
    } else if let Some(e) = &g.power_error {
        Some((e.clone(), Hue::Red))
    } else if g.caps {
        Some(("Caps Lock is on".to_owned(), Hue::Amber))
    } else {
        None
    };
    let label = msg.as_ref().map(|(m, _)| m.clone()).unwrap_or_default();
    drawn(size.width, LINE_H, msg.as_ref().map(|(m, h)| (m.clone(), *h)), move |cx| {
        if let Some((m, hue)) = &msg {
            let night = aizome_kit::Theme::night();
            let s = look::ui(13.0, typeface::WEIGHT_REGULAR, hue.color(&night));
            cx.draw_text_in(m, &s, Rect::new(0.0, 8.0, cx.bounds().x1, 28.0), Align::Center);
        }
    })
    .text(label, SemanticRole::Label)
    .into()
}
