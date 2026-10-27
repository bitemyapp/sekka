//! What a menu tells whoever showed it, besides the item chosen: that it
//! closed (however it closed), and what's typed in it when it hands typing
//! over.

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_kit::{Anchors, KeyboardInteractivity, Layer, LayerOptions};

#[derive(Default)]
struct Bar {
    bar: Option<WindowKey>,
    heard: Vec<String>,
    /// The bar's title whose menu is open, as F10 and the arrows move it.
    title: Option<i32>,
}

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Open(u32),
    /// A launcher's menu: typing in it is a search.
    OpenLauncher,
    CloseAll,
    Closed(u32),
    Choose(&'static str),
    Typed(String),
    /// F10: the bar's first menu, from the keyboard.
    Keyboard,
    /// A bar menu whose submenus tear off.
    OpenTearable,
    TornSub(Vec<usize>, aizome_kit::Vec2),
    Side(i32),
}

fn menu() -> Menu<Msg> {
    Menu::new("File")
        .item(MenuItem::new("New", Msg::Choose("new")))
        .item(MenuItem::new("Open…", Msg::Choose("open")))
        .submenu(Menu::new("Open Recent").item(MenuItem::new("notes.md", Msg::Choose("notes"))))
}

impl Program for Bar {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Test", "Test", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        let options = LayerOptions::new(Layer::Top, "aizome-menubar")
            .anchors(Anchors::TOP | Anchors::LEFT | Anchors::RIGHT)
            .exclusive_zone(24)
            .keyboard(KeyboardInteractivity::OnDemand);
        self.bar = Some(cx.open_window(WindowSpec::layer(options, 600.0, 24.0)));
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        let bar = self.bar.expect("the bar");
        match msg {
            Msg::Open(n) => {
                let x = 40.0 * n as f64;
                cx.show_menu(
                    bar,
                    Rect::new(x, 0.0, x + 40.0, 24.0),
                    menu(),
                    MenuStyle::below().on_closed(Msg::Closed(n)),
                );
            }
            Msg::OpenLauncher => {
                let style = MenuStyle::below().titled("Toolchest").on_type(Msg::Typed).on_closed(Msg::Closed(0));
                cx.show_menu(bar, Rect::new(0.0, 0.0, 40.0, 24.0), menu(), style);
            }
            Msg::CloseAll => cx.close_menus(bar),
            Msg::Closed(n) => self.heard.push(format!("closed {n}")),
            Msg::Choose(c) => self.heard.push(c.to_owned()),
            Msg::Typed(s) => self.heard.push(format!("typed {s}")),
            Msg::OpenTearable => {
                let style = MenuStyle::below().on_tear_off_sub(Msg::TornSub);
                cx.show_menu(bar, Rect::new(0.0, 0.0, 40.0, 24.0), menu(), style);
            }
            Msg::TornSub(path, at) => self.heard.push(format!("torn {path:?} at {:.0},{:.0}", at.x, at.y)),
            Msg::Keyboard | Msg::Side(_) => {
                // Three titles, 40pt apart; the arrows wrap round.
                let i = match (msg, self.title) {
                    (Msg::Side(d), Some(t)) => (t + d).rem_euclid(3),
                    _ => 0,
                };
                self.title = Some(i);
                self.heard.push(format!("title {i}"));
                let x = 40.0 * i as f64;
                let style = MenuStyle::below().from_keyboard().on_side(Msg::Side);
                cx.show_menu(bar, Rect::new(x, 0.0, x + 40.0, 24.0), menu(), style);
            }
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        aizome_kit::row![text("File"), spacer()].padding([0.0, 8.0]).into()
    }
}

fn heard(h: &mut Harness<Bar>) -> Vec<String> {
    std::mem::take(&mut h.program_mut().heard)
}

#[test]
fn a_menu_says_when_it_closes_however_it_closes() {
    let mut h = Harness::new(Bar::default());
    let bar = h.window();

    // Escape.
    h.send(Msg::Open(1));
    assert_eq!(h.popups(bar).len(), 1);
    assert!(heard(&mut h).is_empty(), "open, nothing said yet");
    h.named(bar, NamedKey::Escape, Modifiers::NONE);
    assert!(h.popups(bar).is_empty());
    assert_eq!(heard(&mut h), ["closed 1"]);

    // An item chosen: the item's message, and the menu closing.
    h.send(Msg::Open(1));
    h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(bar, NamedKey::Enter, Modifiers::NONE);
    let said = heard(&mut h);
    assert!(said.contains(&"new".to_owned()) && said.contains(&"closed 1".to_owned()), "{said:?}");

    // Another menu in its place: the first's closing only.
    h.send(Msg::Open(1));
    h.send(Msg::Open(2));
    assert_eq!(h.popups(bar).len(), 1);
    assert_eq!(heard(&mut h), ["closed 1"]);

    // A submenu opening and closing isn't the menu closing.
    for _ in 0..3 {
        h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
    }
    h.named(bar, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(h.popups(bar).len(), 2);
    h.named(bar, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(h.popups(bar).len(), 1);
    assert!(heard(&mut h).is_empty());

    // Closed by the program.
    h.send(Msg::CloseAll);
    assert_eq!(heard(&mut h), ["closed 2"]);

    // A press outside it, in the window it's over.
    h.send(Msg::Open(3));
    h.click(bar, Point::new(500.0, 12.0));
    assert!(h.popups(bar).is_empty());
    assert_eq!(heard(&mut h), ["closed 3"]);
}

#[test]
fn a_launchers_menu_hands_typing_over() {
    let mut h = Harness::new(Bar::default());
    let bar = h.window();
    h.send(Msg::OpenLauncher);
    h.type_text(bar, "s");
    assert!(h.popups(bar).is_empty(), "the menus make way");
    assert_eq!(heard(&mut h), ["typed s", "closed 0"]);

    // From a submenu too; the keys that move and choose still do.
    h.send(Msg::OpenLauncher);
    for _ in 0..3 {
        h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
    }
    h.named(bar, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(h.popups(bar).len(), 2);
    h.type_text(bar, "n");
    assert!(h.popups(bar).is_empty());
    assert_eq!(heard(&mut h), ["typed n", "closed 0"]);

    // Space still chooses the highlighted row.
    h.send(Msg::OpenLauncher);
    h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
    h.type_text(bar, " ");
    let said = heard(&mut h);
    assert!(said.contains(&"new".to_owned()), "{said:?}");

    // Without it, typing filters as ever.
    h.send(Msg::Open(1));
    h.type_text(bar, "rec");
    assert_eq!(h.popups(bar).len(), 1);
    assert!(heard(&mut h).is_empty());
}

#[test]
fn a_bars_menus_follow_the_arrows_across_its_titles() {
    let mut h = Harness::new(Bar::default());
    let bar = h.window();
    h.send(Msg::Keyboard);
    assert_eq!(heard(&mut h), ["title 0"]);
    let (_, at) = h.popups(bar)[0];
    assert_eq!(at.x0, 0.0);
    // Opened from the keyboard, its first row is highlighted already.
    h.named(bar, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(heard(&mut h), ["new"]);
    assert!(h.popups(bar).is_empty());

    // → on a row without a submenu: the next title's menu.
    h.send(Msg::Keyboard);
    h.named(bar, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(heard(&mut h), ["title 0", "title 1"]);
    let popups = h.popups(bar);
    assert_eq!(popups.len(), 1, "one menu at a time");
    assert_eq!(popups[0].1.x0, 40.0);
    // ← from the menu itself: the title before, round to the last.
    h.named(bar, NamedKey::ArrowLeft, Modifiers::NONE);
    h.named(bar, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(heard(&mut h), ["title 0", "title 2"]);

    // In a submenu, ← closes it, and → on its rows goes on to the next title.
    h.named(bar, NamedKey::ArrowUp, Modifiers::NONE);
    h.named(bar, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(h.popups(bar).len(), 2, "Open Recent opened");
    h.named(bar, NamedKey::ArrowLeft, Modifiers::NONE);
    assert_eq!(h.popups(bar).len(), 1);
    assert!(heard(&mut h).is_empty());
    h.named(bar, NamedKey::ArrowRight, Modifiers::NONE);
    h.named(bar, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(heard(&mut h), ["title 0"]);
    assert_eq!(h.popups(bar).len(), 1);
}

#[test]
fn submenus_tear_off_with_their_place() {
    let mut h = Harness::new(Bar::default());
    let bar = h.window();
    h.send(Msg::OpenTearable);
    for _ in 0..3 {
        h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
    }
    h.named(bar, NamedKey::ArrowRight, Modifiers::NONE);
    let popups = h.popups(bar);
    assert_eq!(popups.len(), 2, "Open Recent opened");
    let (sub, r) = popups[1];
    // Its strip has its name, a close box and a pin; the pin tears it off.
    let png = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("submenu-strip.png");
    let img = h.render_popup(sub, 2.0);
    let (w, hh) = img.pixel_size();
    aizome_kit::testing::write_png(&png, w, hh, &img.to_rgba8().expect("pixels")).expect("written");
    h.click_popup(sub, Point::new(r.width() - 3.0 - 12.0, 12.0));
    let said = heard(&mut h);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("torn [2] at "), "{said:?}");
    assert!(h.popups(bar).is_empty(), "the menus closed");
    // Beside its row: right of the menu, as far down as the row.
    let at: Vec<f64> = said[0].trim_start_matches("torn [2] at ").split(',').map(|n| n.parse().unwrap()).collect();
    assert!(at[0] > 150.0 && at[1] > 40.0, "{at:?}");
}

#[test]
fn menus_are_frosted_where_the_effects_are_full() {
    // Inside a menu, away from its text: the face's alpha, and how much it
    // varies (the grain).
    let face = |effects: aizome_kit::theme::Effects| {
        let look = aizome_kit::Appearance { effects, ..Default::default() };
        let mut h = Harness::with(Bar::default(), look, false);
        let bar = h.window();
        h.send(Msg::Open(1));
        let (menu, _) = h.popups(bar)[0];
        let img = h.render_popup(menu, 1.0);
        let (w, _) = img.pixel_size();
        let px = img.to_premultiplied_rgba8().expect("pixels");
        // The right half of the rows' gap under the last row: no text.
        let y = 2u32;
        let alphas: Vec<u8> = (w / 2..w - 10).map(|x| px[((y * w + x) * 4 + 3) as usize]).collect();
        let lumas: Vec<u8> = (w / 2..w - 10).map(|x| px[((y * w + x) * 4) as usize]).collect();
        (alphas, lumas)
    };
    let (alpha, luma) = face(aizome_kit::theme::Effects::Lean);
    assert!(alpha.iter().all(|a| *a == 255), "Lean is opaque");
    assert!(luma.windows(2).all(|p| p[0] == p[1]), "and even");
    let (alpha, luma) = face(aizome_kit::theme::Effects::Full);
    assert!(alpha.iter().all(|a| (200..250).contains(a)), "Full lets what's under show: {alpha:?}");
    assert!(luma.windows(2).any(|p| p[0] != p[1]), "with a grain");
}
