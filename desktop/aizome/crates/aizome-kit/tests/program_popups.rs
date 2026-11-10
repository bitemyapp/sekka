//! Popups of the program's own: a panel under a desktop surface's button,
//! its content the program's view. One that holds a grab is dismissed by a
//! press outside it, in whichever of the program's windows it lands, as a
//! menu or a popover is (`menu_dismiss.rs`): the compositor dismisses it
//! for a press in another program only.

use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::PointerButton;
use aizome_kit::testing::Harness;
use aizome_kit::{Anchors, Layer, LayerOptions, MenuPlacement};

#[derive(Default)]
struct Bar {
    bar: Option<WindowKey>,
    desktop: Option<WindowKey>,
    panel: Option<WindowKey>,
    /// Whether its panels hold a grab.
    grab: bool,
    /// What the program was told, in order.
    heard: Vec<Heard>,
    chosen: Vec<&'static str>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Heard {
    /// The bar's button was pressed.
    Net,
    /// `close_requested`, for this window.
    Closed(WindowKey),
}

#[derive(Clone, Debug)]
enum Msg {
    Open,
    /// A menu of the bar's, at a point of it.
    BarMenu(Point),
    Choose(&'static str),
}

fn menu() -> Menu<Msg> {
    Menu::new("Network")
        .item(MenuItem::new("Join", Msg::Choose("join")))
        .item(MenuItem::new("Forget", Msg::Choose("forget")))
}

impl Bar {
    fn grabbing() -> Bar {
        Bar { grab: true, ..Bar::default() }
    }

    /// The windows `close_requested` was heard for.
    fn closed(&self) -> Vec<WindowKey> {
        self.heard
            .iter()
            .filter_map(|h| match h {
                Heard::Closed(w) => Some(*w),
                Heard::Net => None,
            })
            .collect()
    }
}

impl Program for Bar {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Bar", "Bar", "Test")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        let o = LayerOptions::new(Layer::Top, "bar").anchors(Anchors::TOP | Anchors::LEFT | Anchors::RIGHT);
        self.bar = Some(cx.open_window(WindowSpec::layer(o, 800.0, 24.0)));
        let desktop = LayerOptions::new(Layer::Background, "desktop").anchors(Anchors::ALL);
        self.desktop = Some(cx.open_window(WindowSpec::layer(desktop, 800.0, 600.0)));
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Open => {
                self.heard.push(Heard::Net);
                let spec = WindowSpec::new("panel").size(320.0, 200.0).kind(WindowKind::Popup {
                    parent: self.bar.unwrap(),
                    anchor: Rect::new(500.0, 0.0, 524.0, 24.0),
                    placement: MenuPlacement::Below,
                    grab: self.grab,
                });
                self.panel = Some(cx.open_window(spec));
            }
            Msg::BarMenu(at) => {
                let bar = self.bar.unwrap();
                cx.show_menu(bar, Rect::from_origin_size(at, (1.0, 1.0)), menu(), MenuStyle::at_point());
            }
            Msg::Choose(c) => self.chosen.push(c),
        }
    }
    fn view(&self, w: WindowKey) -> Element<Msg> {
        if Some(w) == self.panel {
            return context_menu(text("Wi-Fi"), menu()).into();
        }
        if Some(w) == self.desktop {
            return text("Desktop").into();
        }
        // The button opens its panel at the press, as a menu bar's does.
        row![mouse_area(text("Net")).on_press(Msg::Open), spacer(), text("21:42")].into()
    }
    fn close_requested(&mut self, w: WindowKey, _cx: &mut Cx<Msg>) -> bool {
        self.heard.push(Heard::Closed(w));
        if self.panel == Some(w) {
            self.panel = None;
        }
        true
    }
    fn quit_when_last_window_closes(&self) -> bool {
        false
    }
}

/// The bar, the desktop, and a panel opened from the bar's button.
fn with_panel(program: Bar) -> (Harness<Bar>, WindowKey, WindowKey, WindowKey) {
    let mut h = Harness::new(program);
    let (bar, desktop) = (h.program().bar.unwrap(), h.program().desktop.unwrap());
    let panel = open(&mut h);
    (h, bar, desktop, panel)
}

/// Press the bar's button; the panel it opened.
fn open(h: &mut Harness<Bar>) -> WindowKey {
    let bar = h.program().bar.unwrap();
    let net = h.find(bar, "Net").expect("the button").center();
    h.click(bar, net);
    let panel = h.program().panel.expect("opened");
    assert!(h.windows().contains(&panel));
    panel
}

fn right_click(h: &mut Harness<Bar>, w: WindowKey, text: &str) {
    let at = h.find(w, text).expect(text).center();
    h.press(w, at, PointerButton::Right, 1);
    h.release(w, at, PointerButton::Right);
}

/// Every menu open in the program, with the window it hangs from.
fn open_menus(h: &Harness<Bar>) -> Vec<(WindowKey, u32)> {
    h.windows().into_iter().flat_map(|w| h.popups(w).into_iter().map(move |(p, _)| (w, p))).collect()
}

#[test]
fn a_program_popup_opens_over_its_parent_and_shows_its_view() {
    let mut h = Harness::new(Bar::grabbing());
    h.send(Msg::Open);
    let panel = h.program().panel.expect("opened");
    let spec = &h.platform.windows.values().last().expect("the popup").spec;
    match &spec.kind {
        aizome_kit::platform::OpenKind::Popup { anchor, grab, .. } => {
            assert_eq!(*anchor, Rect::new(500.0, 0.0, 524.0, 24.0));
            assert!(*grab);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(spec.background, None, "it draws its own ground");
    assert!(h.texts(panel).iter().any(|t| t == "Wi-Fi"));
    assert!(!h.program().quit_when_last_window_closes());
}

#[test]
fn a_press_outside_a_grabbing_popup_dismisses_it_wherever_it_lands() {
    // Elsewhere in its parent: the compositor leaves a press in the
    // program's own surfaces to the program, so this is the kit's doing.
    let (mut h, bar, desktop, panel) = with_panel(Bar::grabbing());
    let clock = h.find(bar, "21:42").expect("the clock").center();
    h.click(bar, clock);
    assert_eq!(h.program().closed(), [panel], "the program heard close_requested");
    assert_eq!(h.program().panel, None);
    assert!(!h.windows().contains(&panel), "and it's closed");

    // In another of the program's windows.
    let panel = open(&mut h);
    h.click(desktop, Point::new(500.0, 400.0));
    assert_eq!(h.program().closed().last(), Some(&panel));
    assert!(!h.windows().contains(&panel));

    // Under a menu of its own, by a press on the desktop: the menu goes
    // with it.
    let panel = open(&mut h);
    right_click(&mut h, panel, "Wi-Fi");
    assert_eq!(h.popups(panel).len(), 1, "its menu");
    h.click(desktop, Point::new(500.0, 400.0));
    assert!(!h.windows().contains(&panel));
    assert!(open_menus(&h).is_empty(), "{:?}", open_menus(&h));
    assert!(h.program().chosen.is_empty(), "and nothing chosen");
}

#[test]
fn a_press_inside_a_grabbing_popup_leaves_it_open() {
    let (mut h, _bar, _desktop, panel) = with_panel(Bar::grabbing());
    h.click(panel, Point::new(300.0, 190.0));
    assert_eq!(h.program().panel, Some(panel));
    assert!(h.windows().contains(&panel));

    // A menu over it is inside it: the menu opening leaves the panel, and
    // so does a press in the menu.
    right_click(&mut h, panel, "Wi-Fi");
    assert_eq!(h.popups(panel).len(), 1, "its menu");
    assert!(h.windows().contains(&panel));
    let (menu, _) = h.popups(panel)[0];
    h.click_popup(menu, Point::new(30.0, 17.0));
    assert_eq!(h.program().chosen, ["join"]);
    assert!(h.windows().contains(&panel));

    // A press in the panel outside its menu closes the menu alone.
    right_click(&mut h, panel, "Wi-Fi");
    h.click(panel, Point::new(300.0, 190.0));
    assert!(h.popups(panel).is_empty());
    assert!(h.windows().contains(&panel));
    assert!(h.program().closed().is_empty(), "nothing was asked to close");
}

#[test]
fn a_press_on_what_opened_it_dismisses_it_before_the_press_is_heard() {
    // The kit doesn't know what opened a popup of the program's own (it
    // opens against a rectangle), so that press dismisses it like any
    // other. The program hears of the closing first: a button that
    // toggles its panel can tell its own press closed it. (This one opens
    // a panel at every press.)
    let (mut h, bar, _desktop, panel) = with_panel(Bar::grabbing());
    let net = h.find(bar, "Net").unwrap().center();
    h.click(bar, net);
    assert_eq!(h.program().heard, [Heard::Net, Heard::Closed(panel), Heard::Net]);
    assert!(!h.windows().contains(&panel), "the first panel is closed");
    let again = h.program().panel.expect("the press opened another");
    assert_ne!(again, panel);
    assert!(h.windows().contains(&again));
}

#[test]
fn a_menu_opening_elsewhere_dismisses_a_grabbing_popup() {
    // One at a time, as with menus and popovers.
    let (mut h, bar, _desktop, panel) = with_panel(Bar::grabbing());
    h.send(Msg::BarMenu(Point::new(100.0, 12.0)));
    assert_eq!(h.program().closed(), [panel]);
    assert!(!h.windows().contains(&panel));
    assert_eq!(h.popups(bar).len(), 1, "the bar's menu is open");
}

#[test]
fn a_popup_without_a_grab_stays() {
    // It didn't ask for presses outside it to close it.
    let (mut h, bar, desktop, panel) = with_panel(Bar::default());
    let clock = h.find(bar, "21:42").expect("the clock").center();
    h.click(bar, clock);
    h.click(desktop, Point::new(500.0, 400.0));
    h.send(Msg::BarMenu(Point::new(100.0, 12.0)));
    assert!(h.program().closed().is_empty());
    assert!(h.windows().contains(&panel));
}
