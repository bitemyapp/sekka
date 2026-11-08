//! A menu taller than its output (#338): the compositor shortens its popup,
//! and the menu scrolls its rows in what it got, by the bands at its ends,
//! the wheel and the keys.

use std::time::Duration;

use aizome_kit::platform::OpenKind;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_kit::testing::Harness;

const ROWS: usize = 40;
/// The height the compositor leaves the menu.
const ROOM: f64 = 300.0;

#[derive(Default)]
struct Long {
    chosen: Vec<usize>,
}

#[derive(Clone, Debug)]
enum Msg {
    Pick(usize),
}

impl Program for Long {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.LongMenu", "Long", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        cx.open_window(WindowSpec::new("Long").size(400.0, 200.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        let Msg::Pick(i) = msg;
        self.chosen.push(i);
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let items: Vec<String> = (0..ROWS).map(|i| format!("Item {i}")).collect();
        aizome_kit::column![dropdown(items, None).placeholder("Pick").pull_down().id("pick").on_select(Msg::Pick)]
            .padding(16.0)
            .into()
    }
}

/// The menu open, shortened by the compositor to `ROOM`: it and its width.
fn open_short(h: &mut Harness<Long>) -> (u32, f64) {
    let w = h.window();
    let pick = h.find_id(w, "pick").unwrap();
    h.click(w, pick.center());
    let (menu, natural) = *h.popups(w).last().expect("its menu");
    assert!(natural.height() > ROOM * 2.0, "all its rows asked for: {natural:?}");
    assert!(
        matches!(h.platform.windows[&menu].spec.kind, OpenKind::Popup { fit: true, .. }),
        "a menu lets the compositor shorten it"
    );
    h.configure_popup(menu, Size::new(natural.width(), ROOM));
    let (_, shown) = *h.popups(w).last().unwrap();
    assert_eq!(shown.height(), ROOM, "it stays as short as it was made");
    (menu, natural.width())
}

/// The last row's middle once scrolled to the end: above the bottom edge
/// (1), padding (4) and the shadow (3).
fn last_row(width: f64) -> Point {
    Point::new(width / 2.0, ROOM - 3.0 - 1.0 - 4.0 - 12.0)
}

#[test]
fn the_bottom_band_is_no_row_until_the_menu_scrolls() {
    let mut h = Harness::new(Long::default());
    let (menu, width) = open_short(&mut h);
    // At the bottom, the band: pressing it chooses nothing.
    h.click_popup(menu, last_row(width) + Vec2::new(0.0, 10.0));
    assert!(h.program().chosen.is_empty(), "{:?}", h.program().chosen);
    assert!(!h.popups(h.window()).is_empty(), "the menu stays open");
}

#[test]
fn the_wheel_scrolls_to_the_last_row() {
    let mut h = Harness::new(Long::default());
    let (menu, width) = open_short(&mut h);
    h.scroll_popup(menu, Point::new(width / 2.0, ROOM / 2.0), Vec2::new(0.0, 10_000.0));
    h.click_popup(menu, last_row(width));
    assert_eq!(h.program().chosen, [ROWS - 1]);
}

#[test]
fn resting_on_the_bottom_band_scrolls_to_the_end() {
    let mut h = Harness::new(Long::default());
    let (menu, width) = open_short(&mut h);
    h.move_in_popup(menu, Point::new(width / 2.0, ROOM - 3.0 - 1.0 - 8.0));
    h.advance(Duration::from_secs(10));
    h.click_popup(menu, last_row(width));
    assert_eq!(h.program().chosen, [ROWS - 1]);
}

#[test]
fn the_keys_bring_the_row_they_reach_into_sight() {
    let mut h = Harness::new(Long::default());
    let (menu, width) = open_short(&mut h);
    h.popup_keyboard(menu, Key::Named(NamedKey::End), Modifiers::default());
    // Under the pointer now: the last row, scrolled up into sight.
    h.click_popup(menu, last_row(width));
    assert_eq!(h.program().chosen, [ROWS - 1]);
}

#[test]
fn a_menu_that_fits_asks_for_all_its_rows() {
    let mut h = Harness::new(Long::default());
    let w = h.window();
    let pick = h.find_id(w, "pick").unwrap();
    h.click(w, pick.center());
    let (menu, natural) = *h.popups(w).last().unwrap();
    h.configure_popup(menu, natural.size());
    assert_eq!(h.popups(w).last().unwrap().1.size(), natural.size());
    // Its last row is where all its rows put it.
    let bottom = Point::new(natural.width() / 2.0, natural.height() - 3.0 - 1.0 - 4.0 - 12.0);
    h.click_popup(menu, bottom);
    assert_eq!(h.program().chosen, [ROWS - 1]);
}
