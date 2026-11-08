//! A press outside a menu closes it, in whichever of the app's windows it
//! lands: the compositor closes menus for a press in another app only. And
//! one menu is open at a time (#207).

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::PointerButton;
use aizome_kit::testing::Harness;
use aizome_kit::{Anchors, Layer, LayerOptions};

#[derive(Default)]
struct Desk {
    bar: Option<WindowKey>,
    desktop: Option<WindowKey>,
    notes: Vec<WindowKey>,
    chosen: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    DesktopMenu(Point),
    Choose(&'static str),
}

fn menu() -> Menu<Msg> {
    Menu::new("Desktop")
        .item(MenuItem::new("New Folder", Msg::Choose("folder")))
        .item(MenuItem::new("Change Wallpaper…", Msg::Choose("wallpaper")))
}

impl Program for Desk {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.example.Desk", "Desk", "Desk")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        let bar = LayerOptions::new(Layer::Top, "bar").anchors(Anchors::TOP | Anchors::LEFT | Anchors::RIGHT);
        self.bar = Some(cx.open_window(WindowSpec::layer(bar, 800.0, 24.0)));
        let desktop = LayerOptions::new(Layer::Background, "desktop").anchors(Anchors::ALL);
        self.desktop = Some(cx.open_window(WindowSpec::layer(desktop, 800.0, 600.0)));
        for title in ["Notes", "More Notes"] {
            self.notes.push(cx.open_window(WindowSpec::new(title).size(300.0, 200.0)));
        }
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            // As the Shell does: the desktop's menu hangs from the bar.
            Msg::DesktopMenu(at) => {
                let bar = self.bar.expect("the bar");
                cx.show_menu(bar, Rect::from_origin_size(at, (1.0, 1.0)), menu(), MenuStyle::at_point());
            }
            Msg::Choose(c) => self.chosen.push(c),
        }
    }
    fn view(&self, w: WindowKey) -> Element<Msg> {
        if Some(w) == self.bar {
            return row![text("Toolchest"), spacer(), text("21:42")].into();
        }
        if Some(w) == self.desktop {
            return mouse_area(text("Desktop")).on_right_click(Msg::DesktopMenu).into();
        }
        context_menu(text("A note"), menu()).into()
    }
}

fn right_click(h: &mut Harness<Desk>, w: WindowKey, text: &str) {
    let at = h.find(w, text).expect(text).center();
    h.press(w, at, PointerButton::Right, 1);
    h.release(w, at, PointerButton::Right);
}

/// Every menu open in the app, with the window it hangs from.
fn open_menus(h: &Harness<Desk>) -> Vec<(WindowKey, u32)> {
    h.windows().into_iter().flat_map(|w| h.popups(w).into_iter().map(move |(p, _)| (w, p))).collect()
}

#[test]
fn a_press_in_another_of_the_apps_windows_closes_its_menu() {
    let mut h = Harness::new(Desk::default());
    let (bar, desktop) = (h.program().bar.unwrap(), h.program().desktop.unwrap());
    let notes = h.program().notes[0];

    right_click(&mut h, desktop, "Desktop");
    assert_eq!(h.popups(bar).len(), 1, "the desktop's menu, over the bar");
    // A click elsewhere on the desktop: not the bar's window, nor another
    // app's, which the compositor would have seen to.
    h.click(desktop, Point::new(500.0, 400.0));
    assert!(open_menus(&h).is_empty(), "{:?}", open_menus(&h));
    assert!(h.program().chosen.is_empty(), "and nothing chosen");

    // In another window of the app.
    right_click(&mut h, desktop, "Desktop");
    h.click(notes, Point::new(150.0, 150.0));
    assert!(open_menus(&h).is_empty());

    // A context menu of a window, by a press on the desktop.
    right_click(&mut h, notes, "A note");
    assert_eq!(h.popups(notes).len(), 1);
    h.click(desktop, Point::new(500.0, 400.0));
    assert!(open_menus(&h).is_empty());

    // A click in the menu itself still chooses.
    right_click(&mut h, desktop, "Desktop");
    let (menu, _) = h.popups(bar)[0];
    h.click_popup(menu, Point::new(30.0, 17.0));
    assert_eq!(h.program().chosen, ["folder"]);
    assert!(open_menus(&h).is_empty());
}

#[test]
fn one_menu_is_open_at_a_time() {
    let mut h = Harness::new(Desk::default());
    let (bar, desktop) = (h.program().bar.unwrap(), h.program().desktop.unwrap());
    let (notes, more) = (h.program().notes[0], h.program().notes[1]);

    right_click(&mut h, notes, "A note");
    right_click(&mut h, more, "A note");
    let open = open_menus(&h);
    assert_eq!(open.iter().map(|(w, _)| *w).collect::<Vec<_>>(), [more], "{open:?}");
    right_click(&mut h, desktop, "Desktop");
    let open = open_menus(&h);
    assert_eq!(open.iter().map(|(w, _)| *w).collect::<Vec<_>>(), [bar], "{open:?}");

    // Opened by the program rather than a press (the Shell's ❖-right-click
    // menu, from the compositor): the other closes as well.
    right_click(&mut h, notes, "A note");
    h.send(Msg::DesktopMenu(Point::new(400.0, 300.0)));
    let open = open_menus(&h);
    assert_eq!(open.iter().map(|(w, _)| *w).collect::<Vec<_>>(), [bar], "{open:?}");
}
