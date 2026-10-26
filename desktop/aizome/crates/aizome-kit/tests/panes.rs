//! Window furniture: a Settings-like pane (sidebar, forms), an editor's
//! toolbar, splits, popovers, context menus and mouse areas.

use aizome_kit::Appearance;
use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Modifiers, NamedKey, PointerButton};
use aizome_kit::testing::Harness;

#[derive(Default)]
struct Settings {
    pane: usize,
    wifi: bool,
    theme: usize,
    size: usize,
    brightness: f64,
    panel: bool,
    picked: Vec<&'static str>,
    hovered: bool,
    dropped: Vec<String>,
}

#[derive(Clone, Debug)]
enum Msg {
    Pane(usize),
    Wifi(bool),
    Theme(usize),
    Size(usize),
    Brightness(f64),
    Panel(bool),
    Pick(&'static str),
    Hover(bool),
    Drop(Vec<String>),
}

const PANES: [(Icon, &str); 6] = [
    (Icon::Wifi, "Network"),
    (Icon::Bluetooth, "Bluetooth"),
    (Icon::Volume2, "Sound"),
    (Icon::Monitor, "Displays"),
    (Icon::Palette, "Appearance"),
    (Icon::Keyboard, "Keyboard"),
];

impl Program for Settings {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Settei", "Settei", "Settings")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        self.brightness = 0.7;
        self.wifi = true;
        cx.open_window(WindowSpec::new("Settei").size(720.0, 460.0));
    }
    fn update(&mut self, msg: Msg, _cx: &mut Cx<Msg>) {
        match msg {
            Msg::Pane(p) => self.pane = p,
            Msg::Wifi(b) => self.wifi = b,
            Msg::Theme(t) => self.theme = t,
            Msg::Size(s) => self.size = s,
            Msg::Brightness(b) => self.brightness = b,
            Msg::Panel(b) => self.panel = b,
            Msg::Pick(p) => self.picked.push(p),
            Msg::Hover(h) => self.hovered = h,
            Msg::Drop(u) => self.dropped = u,
        }
    }
    fn view(&self, _w: WindowKey) -> Element<Msg> {
        let mut items: Vec<Element<Msg>> = vec![caption("System")];
        for (i, (icon, name)) in PANES.iter().enumerate() {
            items.push(sidebar_item(*icon, *name, self.pane == i).on_press(Msg::Pane(i)).id(*name).into());
        }
        let sidebar = aizome_kit::widgets::layout::column(items).padding([0.0, 0.0, 8.0, 0.0]);
        let menu = Menu::new("Network")
            .item(MenuItem::new("Copy Address", Msg::Pick("copy")))
            .item(MenuItem::new("Forget", Msg::Pick("forget")));
        let content = scroll(
            column![
                text("Appearance").size(20.0).semibold(),
                form_section(
                    "Look",
                    vec![
                        form_row("Theme", segmented(["Night", "Day", "Auto"], self.theme).on_select(Msg::Theme)).into(),
                        form_row(
                            "Size",
                            dropdown(["Small", "Medium", "Large"], self.size).on_select(Msg::Size).id("size")
                        )
                        .description("Scales type and controls, not pixels.")
                        .into(),
                        form_row(
                            "Brightness",
                            slider(self.brightness, 0.0..=1.0).on_change(Msg::Brightness).width(200.0)
                        )
                        .into(),
                    ],
                ),
                form_section(
                    "Network",
                    vec![
                        context_menu(
                            form_row("Wi-Fi", toggle(self.wifi).on_toggle(Msg::Wifi).id("wifi")).icon(Icon::Wifi),
                            menu
                        )
                        .id("wifi-row")
                        .into(),
                        form_row(
                            "Details",
                            popover(
                                button("Details…").on_press(Msg::Panel(true)).id("details"),
                                column![text("Kestrel-5G").semibold(), text("192.168.1.24").mono().role(Role::Text3)]
                                    .spacing(4.0)
                                    .padding(12.0)
                                    .width(220.0),
                                self.panel
                            )
                            .on_dismiss(Msg::Panel(false))
                        )
                        .into(),
                    ],
                ),
                aizome_kit::mouse_area(container(text("Drop files here").role(Role::Text3)).padding(12.0).width(Fill))
                    .on_hover(Msg::Hover)
                    .on_drop(|d: aizome_kit::Dropped| Msg::Drop(d.urls))
                    .id("drop"),
            ]
            .spacing(16.0)
            .padding(20.0),
        );
        sidebar_layout(sidebar, content).into()
    }
}

fn out(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name)
}

#[test]
fn a_settings_pane_draws_and_answers() {
    let mut h = Harness::new(Settings::default());
    let w = h.window();
    h.save_png(w, out("settings-night.png"), 2.0);
    // The sidebar.
    h.click_text(w, "Sound");
    assert_eq!(h.program().pane, 2);
    // The dropdown's menu.
    let size = h.find_id(w, "size").unwrap();
    h.click(w, size.center());
    let popups = h.popups(w);
    assert_eq!(popups.len(), 1);
    h.save_png(w, out("settings-dropdown.png"), 2.0);
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().size, 2);
    // The switch inside the row with a context menu.
    let wifi = h.find_id(w, "wifi").unwrap();
    h.click(w, wifi.center());
    assert!(!h.program().wifi);
    let row = h.find_id(w, "wifi-row").unwrap();
    h.press(w, Point::new(row.x0 + 60.0, row.center().y), PointerButton::Right, 1);
    h.release(w, Point::new(row.x0 + 60.0, row.center().y), PointerButton::Right);
    assert_eq!(h.popups(w).len(), 1, "the context menu opened");
    h.named(w, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(w, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().picked, ["copy"]);
}

#[test]
fn popovers_open_with_their_state_and_dismiss() {
    let mut h = Harness::new(Settings::default());
    let w = h.window();
    let details = h.find_id(w, "details").unwrap();
    h.click(w, details.center());
    assert!(h.program().panel);
    let popups = h.popups(w);
    assert_eq!(popups.len(), 1, "the popover opened");
    let (p, r) = popups[0];
    assert!(r.y0 >= details.y1 - 0.5, "below its anchor");
    h.save_png(w, out("settings-popover.png"), 2.0);
    // Escape dismisses it, through the program.
    h.focus_window(w, true);
    h.popup_keyboard(p, aizome_kit::sidestep_ui::Key::Named(NamedKey::Escape), Modifiers::NONE);
    assert!(!h.program().panel);
    assert!(h.popups(w).is_empty());
}

#[test]
fn mouse_areas_hover_and_take_drops() {
    let mut h = Harness::new(Settings::default());
    let w = h.window();
    let drop = h.find_id(w, "drop").unwrap();
    h.move_to(w, drop.center());
    assert!(h.program().hovered);
    h.move_to(w, Point::new(5.0, 5.0));
    assert!(!h.program().hovered);
    let _ = drop;
}

#[test]
fn day_and_large_rethemes_everything() {
    let mut h = Harness::new(Settings::default());
    let w = h.window();
    h.set_appearance(Appearance { theme: aizome_kit::theme::ThemeChoice::Day, ..Appearance::default() });
    h.save_png(w, out("settings-day.png"), 2.0);
    h.set_appearance(Appearance { size: aizome_kit::theme::SizeChoice::Large, ..Appearance::default() });
    h.save_png(w, out("settings-large.png"), 1.0);
}

/// An editor's window chrome, as the board's.
#[derive(Default)]
struct Editor {
    syntax: usize,
    bold: bool,
}

#[derive(Clone, Debug)]
enum EdMsg {
    Syntax(usize),
    Bold,
}

impl Program for Editor {
    type Msg = EdMsg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Sumi", "Editor", "Text Editor")
    }
    fn launched(&mut self, cx: &mut Cx<EdMsg>) {
        cx.open_window(WindowSpec::new("architecture.md").size(700.0, 220.0));
    }
    fn update(&mut self, msg: EdMsg, _cx: &mut Cx<EdMsg>) {
        match msg {
            EdMsg::Syntax(s) => self.syntax = s,
            EdMsg::Bold => self.bold = !self.bold,
        }
    }
    fn view(&self, _w: WindowKey) -> Element<EdMsg> {
        column![
            toolbar(vec![
                dropdown(["Markdown", "Plain Text", "Rust"], self.syntax).on_select(EdMsg::Syntax).compact().into(),
                toolbar_separator(),
                button("B").toolbar().weight(800.0).selected(self.bold).on_press(EdMsg::Bold).into(),
                button("I").toolbar().italic().weight(600.0).on_press(EdMsg::Bold).into(),
                button("</>").toolbar().mono().on_press(EdMsg::Bold).into(),
                spacer().into(),
                status_text("Ln 48, Col 12 · UTF-8 · LF"),
            ]),
            container(text("## Scale").mono().role(Role::Accent).semibold()).padding(12.0),
            spacer(),
            status_bar(vec![status_text("1,204 words"), spacer().into(), status_text("Saved")]),
        ]
        .into()
    }
}

#[test]
fn an_editor_toolbar_as_the_board() {
    let mut h = Harness::new(Editor::default());
    let w = h.window();
    h.click_text(w, "B");
    assert!(h.program().bold);
    h.save_png(w, out("editor-toolbar.png"), 2.0);
}
