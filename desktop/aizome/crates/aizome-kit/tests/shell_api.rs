//! What the shell needs of the kit: desktop surfaces, the session lock,
//! outputs, and menus anchored anywhere, run by the kit.

use aizome_kit::column;
use aizome_kit::prelude::*;
use aizome_kit::sidestep_ui::{Key, Modifiers, NamedKey};
use aizome_kit::testing::Harness;
use aizome_kit::{Anchors, KeyboardInteractivity, Layer, LayerOptions, Margins, Output};

#[derive(Default)]
struct Shell {
    bar: Option<WindowKey>,
    locks: Vec<WindowKey>,
    locked: bool,
    finished: bool,
    chosen: Vec<&'static str>,
    outputs_seen: usize,
    token: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Choose(&'static str),
    Lock,
    Unlock,
    OpenMenu,
    Move(i32),
    TearOff,
}

fn edit_menu() -> Menu<Msg> {
    Menu::new("Edit")
        .item(MenuItem::new("Undo", Msg::Choose("undo")).shortcut(Shortcut::cmd('Z')))
        .item(MenuItem::new("Redo", Msg::Choose("redo")).enabled(false))
        .separator()
        .item(MenuItem::new("Cut", Msg::Choose("cut")))
        .submenu(
            Menu::new("Transformations")
                .item(MenuItem::new("Make Upper Case", Msg::Choose("upper")))
                .item(MenuItem::new("Copy as Plain Text", Msg::Choose("copy-plain"))),
        )
        .item(MenuItem::new("Paste", Msg::Choose("paste")))
}

impl Program for Shell {
    type Msg = Msg;
    fn info(&self) -> AppInfo {
        AppInfo::new("org.aizome.Shell", "Aizome", "Shell")
    }
    fn launched(&mut self, cx: &mut Cx<Msg>) {
        let options = LayerOptions::new(Layer::Top, "aizome-menubar")
            .anchors(Anchors::TOP | Anchors::LEFT | Anchors::RIGHT)
            .exclusive_zone(24)
            .keyboard(KeyboardInteractivity::OnDemand);
        self.bar = Some(cx.open_window(WindowSpec::layer(options, 600.0, 24.0)));
    }
    fn update(&mut self, msg: Msg, cx: &mut Cx<Msg>) {
        match msg {
            Msg::Choose(c) => self.chosen.push(c),
            Msg::Lock => {
                cx.lock_session();
                for o in cx.outputs() {
                    self.locks.push(cx.open_window(WindowSpec::lock(o.id)));
                }
            }
            Msg::Unlock => {
                cx.unlock_session();
                self.locks.clear();
            }
            Msg::OpenMenu => {
                let bar = self.bar.expect("the bar");
                let style = MenuStyle::below().on_tear_off(Msg::TearOff);
                cx.show_menu(bar, Rect::new(40.0, 0.0, 80.0, 24.0), edit_menu(), style);
            }
            Msg::Move(x) => {
                let bar = self.bar.expect("the bar");
                cx.set_layer_margins(bar, Margins::new(0, 0, 0, x));
                cx.set_input_region(bar, Some(vec![Rect::new(0.0, 0.0, 100.0, 24.0)]));
                let _ = cx.request_activation_token(Some("org.aizome.Fusen"));
            }
            Msg::TearOff => self.chosen.push("torn"),
        }
    }
    fn view(&self, w: WindowKey) -> Element<Msg> {
        if self.locks.contains(&w) {
            return column![text("Locked")].into();
        }
        aizome_kit::row![button("Edit").toolbar().on_press(Msg::OpenMenu), spacer(), text("21:42").mono()]
            .padding([0.0, 8.0])
            .into()
    }
    fn outputs_changed(&mut self, _cx: &mut Cx<Msg>) {
        self.outputs_seen += 1;
    }
    fn session_locked(&mut self, _cx: &mut Cx<Msg>) {
        self.locked = true;
    }
    fn session_lock_finished(&mut self, _cx: &mut Cx<Msg>) {
        self.finished = true;
        self.locks.clear();
    }
    fn activation_token(&mut self, _r: aizome_kit::ActivationRequest, token: Option<String>, _cx: &mut Cx<Msg>) {
        self.token = token;
    }
}

fn output(id: u32, name: &str) -> Output {
    Output {
        id,
        name: name.into(),
        connector: Some(name.into()),
        description: None,
        make: "Make".into(),
        model: "Model".into(),
        frame: Rect::new(0.0, 0.0, 1440.0, 900.0),
        scale: 2.0,
        integer_scale: 2,
        physical_size: None,
        refresh_hz: 60.0,
        work_area: None,
    }
}

#[test]
fn layer_surfaces_open_move_and_draw_see_through() {
    let mut h = Harness::new(Shell::default());
    let bar = h.window();
    let spec = &h.platform.windows.values().next().expect("a window").spec;
    assert!(
        matches!(&spec.kind, aizome_kit::platform::OpenKind::Layer(o) if o.namespace == "aizome-menubar" && o.exclusive_zone == 24)
    );
    assert_eq!(spec.background, None, "desktop surfaces draw their own ground");
    h.send(Msg::Move(120));
    assert!(
        h.platform
            .layer_changes
            .iter()
            .any(|(_, c)| *c == aizome_kit::platform::LayerChange::Margins(Margins::new(0, 0, 0, 120)))
    );
    assert_eq!(h.platform.input_regions.values().next(), Some(&Some(vec![Rect::new(0.0, 0.0, 100.0, 24.0)])));
    h.activation_token(1, Some("token-1"));
    assert_eq!(h.program().token.as_deref(), Some("token-1"));
    let (w, _, px) = h.render_rgba(bar, 1.0);
    // Far right of the clock, nothing drew: transparent.
    let at = |x: usize, y: usize| px[(y * w as usize + x) * 4 + 3];
    assert_eq!(at(300, 12), 0, "the root doesn't fill a layer surface");
}

#[test]
fn locking_opens_a_surface_per_output_and_unlocking_closes_them() {
    let mut h = Harness::new(Shell::default());
    h.set_outputs(vec![output(1, "eDP-1"), output(2, "HDMI-A-1")]);
    assert_eq!(h.program().outputs_seen, 1);
    h.send(Msg::Lock);
    assert!(h.platform.lock_asked);
    assert_eq!(h.windows().len(), 3);
    h.confirm_lock();
    assert!(h.program().locked);
    let lock = h.windows()[1];
    assert!(h.texts(lock).iter().any(|t| t == "Locked"));
    h.send(Msg::Unlock);
    assert_eq!(h.windows().len(), 1, "the lock's surfaces closed");
    // A refused lock closes them too.
    h.send(Msg::Lock);
    h.finish_lock();
    assert!(h.program().finished);
    assert_eq!(h.windows().len(), 1);
}

#[test]
fn menus_from_a_surface_answer_the_keyboard() {
    let mut h = Harness::new(Shell::default());
    let bar = h.window();
    h.send(Msg::OpenMenu);
    let popups = h.popups(bar);
    assert_eq!(popups.len(), 1, "the menu opened");
    let (menu, at) = popups[0];
    assert_eq!(at.origin(), Point::new(40.0, 24.0), "below its anchor");
    // ↓ to Undo, ↓ skips the disabled Redo and the separator to Cut.
    h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(bar, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().chosen, ["cut"]);
    assert!(h.popups(bar).is_empty(), "choosing closes the menu");
    // → opens a submenu and gives it the keyboard.
    h.send(Msg::OpenMenu);
    for _ in 0..3 {
        h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
    }
    h.named(bar, NamedKey::ArrowRight, Modifiers::NONE);
    assert_eq!(h.popups(bar).len(), 2, "the submenu opened beside");
    h.named(bar, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().chosen, ["cut", "upper"]);
    assert!(h.popups(bar).is_empty());
    let _ = menu;
}

#[test]
fn typing_in_a_menu_filters_it_through_its_submenus() {
    let mut h = Harness::new(Shell::default());
    let bar = h.window();
    h.send(Msg::OpenMenu);
    let (_, whole) = h.popups(bar)[0];
    h.type_text(bar, "plain");
    let (menu, filtered) = h.popups(bar)[0];
    // It shortens to what's left (widening for the submenus' paths shown),
    // where it was.
    assert!(filtered.height() < whole.height() - 40.0, "{filtered:?} from {whole:?}");
    assert!(filtered.origin() == whole.origin() && filtered.width() >= whole.width());
    assert_eq!(h.platform.requested_sizes.get(&menu), Some(&filtered.size()), "the popup asked for its new size");
    let png = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("menu-filtered.png");
    let img = h.render_popup(menu, 2.0);
    let (w, hh) = img.pixel_size();
    aizome_kit::testing::write_png(&png, w, hh, &img.to_rgba8().expect("pixels")).expect("written");
    h.named(bar, NamedKey::Enter, Modifiers::NONE);
    assert_eq!(h.program().chosen, ["copy-plain"]);
    // Escape clears what's typed, then closes.
    h.send(Msg::OpenMenu);
    h.type_text(bar, "zz");
    h.named(bar, NamedKey::Escape, Modifiers::NONE);
    assert_eq!(h.popups(bar).len(), 1);
    assert_eq!(h.popups(bar)[0].1.height(), whole.height(), "cleared, it grows back");
    h.named(bar, NamedKey::Escape, Modifiers::NONE);
    assert!(h.popups(bar).is_empty());
}

#[test]
fn menus_follow_the_pointer_and_clicks() {
    let mut h = Harness::new(Shell::default());
    let bar = h.window();
    h.send(Msg::OpenMenu);
    let (menu, _) = h.popups(bar)[0];
    // Rows start under the top edge and padding: Undo at y 5..29.
    h.click_popup(menu, Point::new(30.0, 17.0));
    assert_eq!(h.program().chosen, ["undo"]);
    let _ = Key::Named(NamedKey::Tab);
}

#[test]
fn menus_draw_as_the_boards() {
    let mut h = Harness::new(Shell::default());
    let bar = h.window();
    h.send(Msg::OpenMenu);
    let (menu, _) = h.popups(bar)[0];
    h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
    h.named(bar, NamedKey::ArrowDown, Modifiers::NONE);
    let img = h.render_popup(menu, 2.0);
    let (w, hh) = img.pixel_size();
    let png = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("menu.png");
    aizome_kit::testing::write_png(&png, w, hh, &img.to_rgba8().expect("pixels")).expect("written");
}
