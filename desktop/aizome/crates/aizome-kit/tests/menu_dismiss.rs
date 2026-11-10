//! A press outside a menu or a popover dismisses it, in whichever of the
//! app's windows it lands: the compositor dismisses them for a press in
//! another app only. And one menu is open at a time (#207).

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::PointerButton;
use aizome_kit::testing::Harness;
use aizome_kit::{Anchors, Appearance, Layer, LayerOptions};

#[derive(Default)]
struct Desk {
    bar: Option<WindowKey>,
    desktop: Option<WindowKey>,
    notes: Vec<WindowKey>,
    /// A window with a popover: its button opens it, `on_dismiss` closes it.
    panel: Option<WindowKey>,
    details: bool,
    chosen: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    DesktopMenu(Point),
    Choose(&'static str),
    Details(bool),
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
        self.panel = Some(cx.open_window(WindowSpec::new("Panel").size(300.0, 200.0)));
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            // As the Shell does: the desktop's menu hangs from the bar.
            Msg::DesktopMenu(at) => {
                let bar = self.bar.expect("the bar");
                cx.show_menu(bar, Rect::from_origin_size(at, (1.0, 1.0)), menu(), MenuStyle::at_point());
            }
            Msg::Choose(c) => self.chosen.push(c),
            Msg::Details(open) => self.details = open,
        }
    }
    fn view(&self, w: WindowKey) -> Element<Msg> {
        if Some(w) == self.bar {
            return row![text("Toolchest"), spacer(), text("21:42")].into();
        }
        if Some(w) == self.desktop {
            return mouse_area(text("Desktop")).on_right_click(Msg::DesktopMenu).into();
        }
        if Some(w) == self.panel {
            let details = popover(
                button("Details…").on_press(Msg::Details(true)).id("details"),
                container(text("Kestrel-5G")).padding(12.0),
                self.details,
            )
            .on_dismiss(Msg::Details(false));
            return column![details, text("Panel")].padding(10.0).into();
        }
        context_menu(text("A note"), menu()).into()
    }
}

fn right_click(h: &mut Harness<Desk>, w: WindowKey, text: &str) {
    let at = h.find(w, text).expect(text).center();
    h.press(w, at, PointerButton::Right, 1);
    h.release(w, at, PointerButton::Right);
}

/// Open the panel's popover from its button; the panel's window.
fn open_popover(h: &mut Harness<Desk>) -> WindowKey {
    let panel = h.program().panel.expect("the panel");
    let anchor = h.find_id(panel, "details").expect("the button").center();
    h.click(panel, anchor);
    assert!(h.program().details);
    assert_eq!(h.popups(panel).len(), 1, "the popover opened");
    panel
}

/// Every menu or popover open in the app, with the window it hangs from.
fn open_popups(h: &Harness<Desk>) -> Vec<(WindowKey, u32)> {
    h.windows().into_iter().flat_map(|w| h.popups(w).into_iter().map(move |(p, _)| (w, p))).collect()
}

/// The windows with a menu or popover open over them.
fn windows_with_popups(h: &Harness<Desk>) -> Vec<WindowKey> {
    open_popups(h).into_iter().map(|(w, _)| w).collect()
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
    assert!(open_popups(&h).is_empty(), "{:?}", open_popups(&h));
    assert!(h.program().chosen.is_empty(), "and nothing chosen");

    // In another window of the app.
    right_click(&mut h, desktop, "Desktop");
    h.click(notes, Point::new(150.0, 150.0));
    assert!(open_popups(&h).is_empty());

    // A context menu of a window, by a press on the desktop.
    right_click(&mut h, notes, "A note");
    assert_eq!(h.popups(notes).len(), 1);
    h.click(desktop, Point::new(500.0, 400.0));
    assert!(open_popups(&h).is_empty());

    // A click in the menu itself still chooses.
    right_click(&mut h, desktop, "Desktop");
    let (menu, _) = h.popups(bar)[0];
    h.click_popup(menu, Point::new(30.0, 17.0));
    assert_eq!(h.program().chosen, ["folder"]);
    assert!(open_popups(&h).is_empty());
}

#[test]
fn one_menu_is_open_at_a_time() {
    let mut h = Harness::new(Desk::default());
    let (bar, desktop) = (h.program().bar.unwrap(), h.program().desktop.unwrap());
    let (notes, more) = (h.program().notes[0], h.program().notes[1]);

    right_click(&mut h, notes, "A note");
    right_click(&mut h, more, "A note");
    assert_eq!(windows_with_popups(&h), [more], "{:?}", open_popups(&h));
    right_click(&mut h, desktop, "Desktop");
    assert_eq!(windows_with_popups(&h), [bar], "{:?}", open_popups(&h));

    // Opened by the program rather than a press (the Shell's ❖-right-click
    // menu, from the compositor): the other closes as well.
    right_click(&mut h, notes, "A note");
    h.send(Msg::DesktopMenu(Point::new(400.0, 300.0)));
    assert_eq!(windows_with_popups(&h), [bar], "{:?}", open_popups(&h));
}

#[test]
fn a_press_outside_a_popover_dismisses_it_wherever_it_lands() {
    let mut h = Harness::new(Desk::default());
    let (bar, desktop) = (h.program().bar.unwrap(), h.program().desktop.unwrap());

    // Elsewhere in its own window: the compositor leaves a press in the
    // app's own surfaces to the app, so this is the kit's doing.
    let panel = open_popover(&mut h);
    h.click(panel, Point::new(250.0, 180.0));
    assert!(!h.program().details, "the program heard on_dismiss");
    assert!(open_popups(&h).is_empty());

    // On the desktop: another window of the app.
    open_popover(&mut h);
    h.click(desktop, Point::new(500.0, 400.0));
    assert!(!h.program().details);
    assert!(open_popups(&h).is_empty());

    // On its own button: the program's to answer (it opens the popover),
    // not a dismissal, so the popover stays as it is.
    open_popover(&mut h);
    let anchor = h.find_id(panel, "details").unwrap().center();
    h.click(panel, anchor);
    assert!(h.program().details);
    assert_eq!(h.popups(panel).len(), 1, "still the one popover");

    // A menu the program opens elsewhere dismisses it as well.
    h.send(Msg::DesktopMenu(Point::new(400.0, 300.0)));
    assert!(!h.program().details);
    assert_eq!(windows_with_popups(&h), [bar], "{:?}", open_popups(&h));

    // With the desktop's menu up, the button closes that and opens the popover.
    h.click(panel, anchor);
    assert!(h.program().details);
    assert_eq!(windows_with_popups(&h), [panel], "{:?}", open_popups(&h));
}

#[test]
fn a_windows_own_menu_bar_takes_part() {
    // Without the Shell, each window has its menu bar along its top.
    let mut h = Harness::with(Desk::default(), Appearance::default(), true);
    let (bar, desktop) = (h.program().bar.unwrap(), h.program().desktop.unwrap());
    let notes = h.program().notes[0];

    // The desktop's menu is up; the app menu of a window's bar (its name,
    // in bold, at the left) opens in its place.
    right_click(&mut h, desktop, "Desktop");
    assert_eq!(windows_with_popups(&h), [bar]);
    h.click(notes, Point::new(20.0, 12.0));
    assert_eq!(windows_with_popups(&h), [notes], "{:?}", open_popups(&h));

    // And a press on the desktop closes it.
    h.click(desktop, Point::new(500.0, 400.0));
    assert!(open_popups(&h).is_empty(), "{:?}", open_popups(&h));
}
