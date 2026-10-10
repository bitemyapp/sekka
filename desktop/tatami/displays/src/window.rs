// SPDX-License-Identifier: MIT OR Apache-2.0
//! The Displays window, after macOS's Displays settings: the arrangement,
//! then the selected display's settings, then Night Shift. Every change is
//! kept and applied at once by the `tatami` helper. Changes that can leave
//! a screen blank (resolution, refresh rate, rotation, color depth, HDR,
//! mirroring and turning a display off) ask to be kept, and are undone
//! after a countdown otherwise.
use crate::{
    arrangement::{Arrangement, Tile},
    model::{self, Display, Mode, Rect, Settings, Use},
    system,
};
use adw::prelude::*;
use gtk::glib;
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

const CONFIRM_SECONDS: u32 = 15;

const VRR: [&str; 4] = [
    "Off",
    "On",
    "Full-screen windows",
    "Full-screen games and video",
];

const ROTATIONS: [&str; 8] = [
    "Standard",
    "90°",
    "180°",
    "270°",
    "Flipped",
    "Flipped, 90°",
    "Flipped, 180°",
    "Flipped, 270°",
];

/// Color profiles outside HDR (Hyprland's color management presets).
const PROFILES: [(&str, &str); 7] = [
    ("srgb", "Standard (sRGB)"),
    ("auto", "Automatic"),
    ("edid", "The display's own (EDID)"),
    ("wide", "Wide gamut (BT.2020)"),
    ("dcip3", "DCI-P3"),
    ("dp3", "Display P3"),
    ("adobe", "Adobe RGB"),
];

struct State {
    displays: Vec<Display>,
    kept: Value,
    default_vrr: i64,
    selected: String,
    show_all: bool,
}

struct Ui {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    arrangement: Arrangement,
    chooser: gtk::Box,
    content: gtk::Box,
    state: RefCell<State>,
}

pub fn build(app: &adw::Application) -> adw::ApplicationWindow {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Displays")
        .icon_name("preferences-desktop-display")
        .default_width(880)
        .default_height(800)
        .build();
    let arrangement = Arrangement::new();
    let hint = gtk::Label::new(Some(
        "Drag displays to arrange them. Click one to change its settings.",
    ));
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    let chooser = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    chooser.add_css_class("linked");
    chooser.set_halign(gtk::Align::Center);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
    column.set_margin_top(18);
    column.set_margin_bottom(24);
    column.set_margin_start(18);
    column.set_margin_end(18);
    column.append(arrangement.widget());
    column.append(&hint);
    column.append(&chooser);
    column.append(&content);
    content.set_margin_top(12);
    let clamp = adw::Clamp::builder()
        .maximum_size(760)
        .child(&column)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&scroller));
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&toasts));
    window.set_content(Some(&view));
    // Escape closes it, as it does Tatami's other floating windows.
    let shortcuts = gtk::ShortcutController::new();
    shortcuts.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::NamedAction::new("window.close")),
    ));
    window.add_controller(shortcuts);

    let ui = Rc::new(Ui {
        window: window.clone(),
        toasts,
        arrangement: arrangement.clone(),
        chooser,
        content,
        state: RefCell::new(State {
            displays: vec![],
            kept: Value::Null,
            default_vrr: 0,
            selected: String::new(),
            show_all: false,
        }),
    });
    let weak = Rc::downgrade(&ui);
    arrangement.connect_select(move |name| {
        if let Some(ui) = weak.upgrade() {
            select(&ui, name);
        }
    });
    let weak = Rc::downgrade(&ui);
    arrangement.connect_moved(move |name, x, y| {
        if let Some(ui) = weak.upgrade() {
            arrange(&ui, name.to_owned(), x, y);
        }
    });
    // The window keeps the state its controls reach through weak references.
    let owned = ui.clone();
    window.connect_close_request(move |_| {
        let _ = &owned;
        glib::Propagation::Proceed
    });
    refresh(&ui);
    let (sender, receiver) = async_channel::unbounded();
    system::watch(sender);
    let weak = Rc::downgrade(&ui);
    glib::spawn_future_local(async move {
        while receiver.recv().await.is_ok() {
            let Some(ui) = weak.upgrade() else {
                break;
            };
            later(&ui, 300, refresh);
        }
    });
    window
}

fn later(ui: &Rc<Ui>, milliseconds: u64, then: fn(&Rc<Ui>)) {
    let ui = ui.clone();
    glib::timeout_add_local_once(Duration::from_millis(milliseconds), move || then(&ui));
}

fn toast(ui: &Ui, text: &str) {
    ui.toasts.add_toast(adw::Toast::new(text));
}

/// Read the displays again from Hyprland.
fn refresh(ui: &Rc<Ui>) {
    match system::displays() {
        Ok(displays) => {
            let mut state = ui.state.borrow_mut();
            state.kept = system::kept();
            state.default_vrr = system::default_vrr();
            if !displays
                .iter()
                .any(|display| display.name == state.selected)
            {
                state.selected = model::main_display(&displays, &state.kept)
                    .or_else(|| displays.first().map(|display| display.name.clone()))
                    .unwrap_or_default();
            }
            state.displays = displays;
        }
        Err(error) => toast(ui, &error),
    }
    render(ui);
}

fn settings(state: &State) -> Vec<Settings> {
    state
        .displays
        .iter()
        .map(|display| {
            let vrr =
                model::kept_vrr(&state.kept, display, &state.displays).unwrap_or(state.default_vrr);
            Settings::of(display, vrr)
        })
        .collect()
}

fn main_of(state: &State) -> Option<String> {
    model::main_display(&state.displays, &state.kept)
}

fn select(ui: &Rc<Ui>, name: &str) {
    ui.state.borrow_mut().selected = name.to_owned();
    let ui = ui.clone();
    glib::idle_add_local_once(move || render(&ui));
}

/// Change the settings, after the signal that asked for it has returned (the
/// window is drawn again, and the widget that emitted it replaced).
fn change(
    ui: &Rc<Ui>,
    confirm: bool,
    edit: impl FnOnce(&mut Vec<Settings>, &mut Option<String>) + 'static,
) {
    let ui = ui.clone();
    glib::idle_add_local_once(move || {
        let (before, before_main) = {
            let state = ui.state.borrow();
            (settings(&state), main_of(&state))
        };
        let mut after = before.clone();
        let mut main = before_main.clone();
        edit(&mut after, &mut main);
        commit(&ui, &before, &before_main, &after, &main, confirm);
    });
}

/// What a display is now, as far as the window knows until Hyprland reports
/// it.
fn assume(display: &mut Display, settings: &Settings) {
    display.enabled = settings.enabled;
    display.mode = settings.mode;
    display.x = settings.x;
    display.y = settings.y;
    display.scale = settings.scale;
    display.transform = settings.transform;
    display.mirror_of = settings.mirror.clone();
    display.cm = settings.cm.clone();
    display.ten_bit = settings.bitdepth == 10;
    display.sdr_brightness = settings.sdr_brightness;
    display.sdr_saturation = settings.sdr_saturation;
}

fn commit(
    ui: &Rc<Ui>,
    before: &[Settings],
    before_main: &Option<String>,
    after: &[Settings],
    main: &Option<String>,
    confirm: bool,
) -> bool {
    let names = model::changed(before, after);
    if names.is_empty() && main == before_main {
        render(ui);
        return true;
    }
    match system::apply(&model::change_json(after, main.as_deref(), &names)) {
        Ok(()) => {
            {
                let mut state = ui.state.borrow_mut();
                for settings in after {
                    if let Some(display) = state
                        .displays
                        .iter_mut()
                        .find(|display| display.name == settings.name)
                    {
                        assume(display, settings);
                    }
                }
                state.kept = system::kept();
            }
            render(ui);
            // Hyprland's own report, once it has applied the change. In a
            // dry run nothing changes: the window keeps what it assumed.
            if !system::dry_run() {
                later(ui, 800, refresh);
            }
            if confirm {
                ask_to_keep(ui, before.to_vec(), before_main.clone(), names);
            }
            true
        }
        Err(error) => {
            toast(ui, &error);
            refresh(ui);
            false
        }
    }
}

fn countdown(seconds: u32) -> String {
    match seconds {
        1 => "Reverting in 1 second.".to_owned(),
        seconds => format!("Reverting in {seconds} seconds."),
    }
}

/// Keep a change, or go back to the settings before it: by choice, or when
/// the countdown ends because the screen cannot show the question.
fn ask_to_keep(
    ui: &Rc<Ui>,
    before: Vec<Settings>,
    before_main: Option<String>,
    names: Vec<String>,
) {
    let dialog = adw::AlertDialog::new(
        Some("Keep these display settings?"),
        Some(&countdown(CONFIRM_SECONDS)),
    );
    dialog.add_responses(&[("revert", "Revert"), ("keep", "Keep Changes")]);
    dialog.set_response_appearance("keep", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("keep"));
    dialog.set_close_response("revert");
    let settled = Rc::new(Cell::new(false));
    let revert = {
        let ui = ui.clone();
        let settled = settled.clone();
        Rc::new(move || {
            if settled.replace(true) {
                return;
            }
            let current = settings(&ui.state.borrow());
            let main = main_of(&ui.state.borrow());
            // Back to the settings before, for the displays that changed.
            let mut back = current.clone();
            for previous in &before {
                if names.contains(&previous.name)
                    && let Some(slot) = back.iter_mut().find(|s| s.name == previous.name)
                {
                    *slot = previous.clone();
                }
            }
            commit(&ui, &current, &main, &back, &before_main, false);
        })
    };
    let left = Rc::new(Cell::new(CONFIRM_SECONDS));
    {
        let dialog = dialog.clone();
        let settled = settled.clone();
        let revert = revert.clone();
        glib::timeout_add_seconds_local(1, move || {
            if settled.get() {
                return glib::ControlFlow::Break;
            }
            let seconds = left.get().saturating_sub(1);
            left.set(seconds);
            if seconds == 0 {
                dialog.force_close();
                revert();
                return glib::ControlFlow::Break;
            }
            dialog.set_body(&countdown(seconds));
            glib::ControlFlow::Continue
        });
    }
    dialog.connect_response(None, move |_, response| {
        if response == "keep" {
            settled.set(true);
        } else {
            revert();
        }
    });
    dialog.present(Some(&ui.window));
}

/// A display dropped in the arrangement: beside the others.
fn arrange(ui: &Rc<Ui>, name: String, x: i32, y: i32) {
    change(ui, false, move |settings, _| {
        model::place(settings, &name, x, y)
    });
}

/// Change one display's settings.
fn edit_display(
    ui: &Rc<Ui>,
    name: &str,
    confirm: bool,
    edit: impl FnOnce(&mut Settings) + 'static,
) {
    let name = name.to_owned();
    change(ui, confirm, move |settings, _| {
        let Some(index) = settings.iter().position(|s| s.name == name) else {
            return;
        };
        let old = settings[index].logical_size();
        edit(&mut settings[index]);
        if settings[index].logical_size() != old {
            model::resized(settings, &name, old);
        }
    });
}

fn clear(container: &gtk::Box) {
    while let Some(child) = container.first_child() {
        container.remove(&child);
    }
}

/// Draw the window again from the state.
fn render(ui: &Rc<Ui>) {
    let state = ui.state.borrow();
    let main = main_of(&state);
    let tiles = state
        .displays
        .iter()
        .filter(|display| display.enabled && display.mirror_of.is_none())
        .map(|display| {
            let (w, h) = display.logical_size();
            let mirrors: Vec<String> = state
                .displays
                .iter()
                .filter(|other| other.mirror_of.as_deref() == Some(display.name.as_str()))
                .map(Display::title)
                .collect();
            let mut subtitle = model::size_label(w, h);
            if !mirrors.is_empty() {
                subtitle = format!("{subtitle} · also on {}", mirrors.join(", "));
            }
            Tile {
                name: display.name.clone(),
                rect: Rect {
                    x: display.x,
                    y: display.y,
                    w,
                    h,
                },
                title: display.title(),
                subtitle,
                main: main.as_deref() == Some(display.name.as_str()),
            }
        })
        .collect();
    ui.arrangement.set_tiles(tiles, &state.selected);

    clear(&ui.chooser);
    if state.displays.len() > 1 {
        let mut first: Option<gtk::ToggleButton> = None;
        for display in &state.displays {
            let label = if display.enabled {
                display.title()
            } else {
                format!("{} (Off)", display.title())
            };
            let button = gtk::ToggleButton::with_label(&label);
            button.set_tooltip_text(Some(&display.name));
            if let Some(first) = &first {
                button.set_group(Some(first));
            } else {
                first = Some(button.clone());
            }
            button.set_active(display.name == state.selected);
            let weak = Rc::downgrade(ui);
            let name = display.name.clone();
            button.connect_toggled(move |button| {
                if button.is_active()
                    && let Some(ui) = weak.upgrade()
                {
                    select(&ui, &name);
                }
            });
            ui.chooser.append(&button);
        }
    }

    clear(&ui.content);
    if let Some(display) = state
        .displays
        .iter()
        .find(|display| display.name == state.selected)
    {
        for group in display_groups(ui, &state, display, main.as_deref()) {
            ui.content.append(&group);
        }
    }
    ui.content.append(&night_shift_group());
}

fn describe(display: &Display) -> String {
    let mut parts = vec![];
    if let Some(inches) = display.diagonal_inches() {
        parts.push(format!("{inches:.0}-inch display"));
    }
    parts.push(display.name.clone());
    if display.enabled {
        parts.push(format!(
            "{} at {}",
            model::size_label(display.mode.width, display.mode.height),
            model::refresh_label(display.mode.refresh)
        ));
    } else {
        parts.push("off".to_owned());
    }
    parts.join(" · ")
}

fn combo(title: &str, labels: &[String], selected: usize) -> adw::ComboRow {
    let labels: Vec<&str> = labels.iter().map(String::as_str).collect();
    let row = adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&labels))
        .build();
    row.set_selected(selected.min(labels.len().saturating_sub(1)) as u32);
    row
}

fn display_groups(
    ui: &Rc<Ui>,
    state: &State,
    display: &Display,
    main: Option<&str>,
) -> Vec<gtk::Widget> {
    let name = display.name.clone();
    let vrr = model::kept_vrr(&state.kept, display, &state.displays).unwrap_or(state.default_vrr);
    let current = Settings::of(display, vrr);
    let mut groups: Vec<gtk::Widget> = vec![];

    // The display, and what it is used as.
    let header = adw::PreferencesGroup::new();
    header.set_title(&display.title());
    header.set_description(Some(&describe(display)));
    let mut uses = vec![("Main display".to_owned(), Use::Main)];
    if state.displays.len() > 1 {
        uses.push(("Extended display".to_owned(), Use::Extended));
    }
    for source in state
        .displays
        .iter()
        .filter(|other| other.name != name && other.enabled && other.mirror_of.is_none())
    {
        uses.push((
            format!("Mirror for {}", source.title()),
            Use::Mirror(source.name.clone()),
        ));
    }
    let others_on = state
        .displays
        .iter()
        .any(|other| other.name != name && other.enabled);
    if others_on || !display.enabled {
        uses.push(("Off".to_owned(), Use::Off));
    }
    let now = if !display.enabled {
        Use::Off
    } else if let Some(source) = &display.mirror_of {
        Use::Mirror(source.clone())
    } else if main == Some(name.as_str()) {
        Use::Main
    } else {
        Use::Extended
    };
    let labels: Vec<String> = uses.iter().map(|(label, _)| label.clone()).collect();
    let row = combo(
        "Use as",
        &labels,
        uses.iter().position(|(_, used)| *used == now).unwrap_or(0),
    );
    row.set_subtitle(match now {
        Use::Off => "Off until you turn it on or log in again",
        _ => "The main display is where the pointer starts and workspace 1 opens",
    });
    let weak = Rc::downgrade(ui);
    let target = name.clone();
    row.connect_selected_notify(move |row| {
        let Some(ui) = weak.upgrade() else { return };
        let Some((_, used)) = uses.get(row.selected() as usize).cloned() else {
            return;
        };
        let name = target.clone();
        change(&ui, true, move |settings, main| {
            model::use_as(settings, main, &name, used)
        });
    });
    header.add(&row);
    groups.push(header.upcast());

    if !display.enabled {
        return groups;
    }

    // Resolution: scaled sizes first, as in macOS, or every one.
    let resolution = adw::PreferencesGroup::builder().title("Resolution").build();
    let show_all = gtk::Switch::new();
    show_all.set_active(state.show_all);
    show_all.set_valign(gtk::Align::Center);
    let suffix = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let show_label = gtk::Label::new(Some("Show all resolutions"));
    show_label.add_css_class("dim-label");
    suffix.append(&show_label);
    suffix.append(&show_all);
    resolution.set_header_suffix(Some(&suffix));
    let weak = Rc::downgrade(ui);
    show_all.connect_active_notify(move |switch| {
        if let Some(ui) = weak.upgrade() {
            ui.state.borrow_mut().show_all = switch.is_active();
            let ui = ui.clone();
            glib::idle_add_local_once(move || render(&ui));
        }
    });
    if state.show_all {
        let sizes = display.resolutions();
        let labels: Vec<String> = sizes
            .iter()
            .map(|(w, h)| model::size_label(*w, *h))
            .collect();
        let row = combo(
            "Resolution",
            &labels,
            sizes
                .iter()
                .position(|size| *size == (display.mode.width, display.mode.height))
                .unwrap_or(0),
        );
        let weak = Rc::downgrade(ui);
        let (target, modes) = (name.clone(), display.modes.clone());
        row.connect_selected_notify(move |row| {
            let Some(ui) = weak.upgrade() else { return };
            let Some((width, height)) = sizes.get(row.selected() as usize).copied() else {
                return;
            };
            let modes = modes.clone();
            edit_display(&ui, &target, true, move |s| {
                let rates = model::refresh_rates(&modes, width, height);
                let refresh = model::nearest(&rates, s.mode.refresh)
                    .map(|index| rates[index])
                    .unwrap_or(s.mode.refresh);
                s.mode = Mode {
                    width,
                    height,
                    refresh,
                };
                s.scale = model::clean_scale(s.scale, width, height);
            });
        });
        resolution.add(&row);
        let scales = model::valid_scales(display.mode.width, display.mode.height);
        let labels: Vec<String> = scales
            .iter()
            .map(|scale| {
                let (w, h) = model::logical_size(&display.mode, *scale, 0);
                format!(
                    "{} — looks like {}",
                    model::scale_label(*scale),
                    model::size_label(w, h)
                )
            })
            .collect();
        let row = combo(
            "Scale",
            &labels,
            scales
                .iter()
                .position(|scale| (scale - display.scale).abs() < 1e-6)
                .unwrap_or(0),
        );
        let weak = Rc::downgrade(ui);
        let target = name.clone();
        row.connect_selected_notify(move |row| {
            let Some(ui) = weak.upgrade() else { return };
            let Some(scale) = scales.get(row.selected() as usize).copied() else {
                return;
            };
            edit_display(&ui, &target, false, move |s| s.scale = scale);
        });
        resolution.add(&row);
    } else {
        resolution.add(&scale_choices(ui, display));
    }
    groups.push(resolution.upcast());

    // Refresh rate, variable refresh rate and rotation.
    let picture = adw::PreferencesGroup::new();
    let rates = display.refresh_rates();
    let labels: Vec<String> = rates
        .iter()
        .map(|rate| model::refresh_label(*rate))
        .collect();
    let row = combo(
        "Refresh rate",
        &labels,
        model::nearest(&rates, display.mode.refresh).unwrap_or(0),
    );
    let weak = Rc::downgrade(ui);
    let target = name.clone();
    row.connect_selected_notify(move |row| {
        let Some(ui) = weak.upgrade() else { return };
        let Some(refresh) = rates.get(row.selected() as usize).copied() else {
            return;
        };
        edit_display(&ui, &target, true, move |s| s.mode.refresh = refresh);
    });
    picture.add(&row);
    let labels: Vec<String> = VRR.iter().map(|label| (*label).to_owned()).collect();
    let row = combo(
        "Variable refresh rate",
        &labels,
        current.vrr.clamp(0, 3) as usize,
    );
    row.set_subtitle(if display.vrr_active {
        "Adaptive sync is active now"
    } else {
        "Adaptive sync (FreeSync, G-Sync Compatible)"
    });
    let weak = Rc::downgrade(ui);
    let target = name.clone();
    row.connect_selected_notify(move |row| {
        let Some(ui) = weak.upgrade() else { return };
        let vrr = row.selected() as i64;
        edit_display(&ui, &target, true, move |s| s.vrr = vrr);
    });
    picture.add(&row);
    let labels: Vec<String> = ROTATIONS.iter().map(|label| (*label).to_owned()).collect();
    let row = combo("Rotation", &labels, display.transform.clamp(0, 7) as usize);
    let weak = Rc::downgrade(ui);
    let target = name.clone();
    row.connect_selected_notify(move |row| {
        let Some(ui) = weak.upgrade() else { return };
        let transform = row.selected() as i32;
        edit_display(&ui, &target, true, move |s| s.transform = transform);
    });
    picture.add(&row);
    groups.push(picture.upcast());

    groups.push(color_group(ui, display, &current).upcast());

    if display.is_builtin()
        && let Some(device) = system::backlight()
        && let Some(level) = system::brightness(&device)
    {
        let group = adw::PreferencesGroup::builder().title("Brightness").build();
        let row = adw::ActionRow::builder().title("Brightness").build();
        let slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 1.0, 100.0, 1.0);
        slider.set_value(level);
        slider.set_hexpand(true);
        slider.set_size_request(280, -1);
        slider.set_draw_value(false);
        let pending = Rc::new(RefCell::new(None));
        slider.connect_value_changed(move |slider| {
            let (device, percent) = (device.clone(), slider.value());
            debounce(&pending, 40, move || {
                system::set_brightness(&device, percent)
            });
        });
        row.add_suffix(&slider);
        group.add(&row);
        groups.push(group.upcast());
    }
    groups
}

/// The scaled sizes as macOS offers them: from larger text to more space,
/// each as a small screen of that size.
fn scale_choices(ui: &Rc<Ui>, display: &Display) -> gtk::Box {
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("card");
    card.add_css_class("scale-card");
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_homogeneous(true);
    let scales = model::scale_choices(display.mode.width, display.mode.height, display.scale);
    let widest = scales
        .iter()
        .map(|scale| model::logical_size(&display.mode, *scale, 0).0)
        .max()
        .unwrap_or(1) as f64;
    let mut first: Option<gtk::ToggleButton> = None;
    for scale in scales {
        let (w, h) = model::logical_size(&display.mode, scale, 0);
        let button = gtk::ToggleButton::new();
        button.add_css_class("flat");
        button.add_css_class("scale-choice");
        button.set_tooltip_text(Some(&format!(
            "Looks like {} ({})",
            model::size_label(w, h),
            model::scale_label(scale)
        )));
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let screen = gtk::Box::new(gtk::Orientation::Vertical, 0);
        screen.add_css_class("scale-screen");
        let width = (24.0 + 52.0 * w as f64 / widest).round() as i32;
        screen.set_size_request(width, (width as f64 * h as f64 / w as f64).round() as i32);
        screen.set_halign(gtk::Align::Center);
        screen.set_valign(gtk::Align::End);
        // A fixed height, so the screens stand on one line. The screen takes
        // it (at its own size, at the bottom); the frame says it does not
        // expand, so neither does the card.
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 0);
        frame.set_size_request(-1, 52);
        frame.set_vexpand(false);
        frame.append(&screen);
        screen.set_vexpand(true);
        let label = gtk::Label::new(Some(&model::size_label(w, h)));
        label.add_css_class("caption");
        inner.append(&frame);
        inner.append(&label);
        button.set_child(Some(&inner));
        if let Some(first) = &first {
            button.set_group(Some(first));
        } else {
            first = Some(button.clone());
        }
        button.set_active((scale - display.scale).abs() < 1e-6);
        let weak = Rc::downgrade(ui);
        let name = display.name.clone();
        button.connect_toggled(move |button| {
            if button.is_active()
                && let Some(ui) = weak.upgrade()
            {
                edit_display(&ui, &name, false, move |s| s.scale = scale);
            }
        });
        row.append(&button);
    }
    let captions = gtk::CenterBox::new();
    let larger = gtk::Label::new(Some("Larger Text"));
    let more = gtk::Label::new(Some("More Space"));
    for label in [&larger, &more] {
        label.add_css_class("dim-label");
        label.add_css_class("caption");
    }
    captions.set_start_widget(Some(&larger));
    captions.set_end_widget(Some(&more));
    card.append(&row);
    card.append(&captions);
    card
}

fn color_group(ui: &Rc<Ui>, display: &Display, current: &Settings) -> adw::PreferencesGroup {
    let name = display.name.clone();
    let group = adw::PreferencesGroup::builder().title("Color").build();
    let hdr = adw::SwitchRow::builder()
        .title("High Dynamic Range")
        .subtitle("Brighter highlights and a wider range of color, on HDR displays")
        .active(current.is_hdr())
        .build();
    let weak = Rc::downgrade(ui);
    let target = name.clone();
    hdr.connect_active_notify(move |row| {
        let Some(ui) = weak.upgrade() else { return };
        let on = row.is_active();
        edit_display(&ui, &target, true, move |s| {
            if on {
                s.cm = "hdr".to_owned();
                s.bitdepth = 10;
            } else {
                s.cm = "srgb".to_owned();
            }
        });
    });
    group.add(&hdr);
    if current.is_hdr() {
        type Set = fn(&mut Settings, f64);
        let brightness: Set = |s, level| s.sdr_brightness = level;
        let saturation: Set = |s, level| s.sdr_saturation = level;
        for (title, value, set) in [
            (
                "Brightness of SDR content",
                current.sdr_brightness,
                brightness,
            ),
            (
                "Saturation of SDR content",
                current.sdr_saturation,
                saturation,
            ),
        ] {
            let row = adw::ActionRow::builder().title(title).build();
            let slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.5, 2.0, 0.05);
            slider.set_value(value);
            slider.add_mark(1.0, gtk::PositionType::Bottom, None);
            slider.set_hexpand(true);
            slider.set_size_request(240, -1);
            slider.set_draw_value(false);
            let pending = Rc::new(RefCell::new(None));
            let weak = Rc::downgrade(ui);
            let target = name.clone();
            slider.connect_value_changed(move |slider| {
                let level = (slider.value() * 100.0).round() / 100.0;
                let (weak, target) = (weak.clone(), target.clone());
                debounce(&pending, 250, move || {
                    if let Some(ui) = weak.upgrade() {
                        quietly(&ui, &target, move |s| set(s, level));
                    }
                });
            });
            row.add_suffix(&slider);
            group.add(&row);
        }
    } else {
        let labels: Vec<String> = PROFILES
            .iter()
            .map(|(_, label)| (*label).to_owned())
            .collect();
        let row = combo(
            "Color profile",
            &labels,
            PROFILES
                .iter()
                .position(|(cm, _)| *cm == current.cm)
                .unwrap_or(0),
        );
        let weak = Rc::downgrade(ui);
        let target = name.clone();
        row.connect_selected_notify(move |row| {
            let Some(ui) = weak.upgrade() else { return };
            let Some((cm, _)) = PROFILES.get(row.selected() as usize) else {
                return;
            };
            edit_display(&ui, &target, false, move |s| s.cm = (*cm).to_owned());
        });
        group.add(&row);
    }
    let depth = adw::SwitchRow::builder()
        .title("10-bit color")
        .subtitle("Smoother gradients, on displays that support it")
        .active(current.bitdepth == 10)
        .build();
    let weak = Rc::downgrade(ui);
    let target = name;
    depth.connect_active_notify(move |row| {
        let Some(ui) = weak.upgrade() else { return };
        let bitdepth = if row.is_active() { 10 } else { 8 };
        edit_display(&ui, &target, true, move |s| s.bitdepth = bitdepth);
    });
    group.add(&depth);
    group
}

/// A change from a slider: applied without drawing the window again, which
/// would take the slider from under the pointer.
fn quietly(ui: &Rc<Ui>, name: &str, edit: impl FnOnce(&mut Settings)) {
    let (before, main) = {
        let state = ui.state.borrow();
        (settings(&state), main_of(&state))
    };
    let mut after = before.clone();
    if let Some(s) = after.iter_mut().find(|s| s.name == name) {
        edit(s);
    }
    let names = model::changed(&before, &after);
    if names.is_empty() {
        return;
    }
    match system::apply(&model::change_json(&after, main.as_deref(), &names)) {
        Ok(()) => {
            let mut state = ui.state.borrow_mut();
            for settings in &after {
                if let Some(display) = state.displays.iter_mut().find(|d| d.name == settings.name) {
                    assume(display, settings);
                }
            }
        }
        Err(error) => toast(ui, &error),
    }
}

/// Run `then` once the value has stopped changing for `milliseconds`.
fn debounce(
    pending: &Rc<RefCell<Option<glib::SourceId>>>,
    milliseconds: u64,
    then: impl FnOnce() + 'static,
) {
    if let Some(source) = pending.borrow_mut().take() {
        source.remove();
    }
    let slot = pending.clone();
    let source = glib::timeout_add_local_once(Duration::from_millis(milliseconds), move || {
        slot.borrow_mut().take();
        then();
    });
    *pending.borrow_mut() = Some(source);
}

/// Night Shift, as in macOS: warmer colors, at a chosen temperature
/// (hyprsunset, through `tatami nightlight`).
fn night_shift_group() -> adw::PreferencesGroup {
    let (tinted, night) = system::night_shift();
    let group = adw::PreferencesGroup::builder()
        .title("Night Shift")
        .description("Shifts the colors of the screens to the warmer end of the spectrum")
        .build();
    let switch = adw::SwitchRow::builder()
        .title("Night Shift")
        .active(tinted.is_some())
        .build();
    let row = adw::ActionRow::builder().title("Color temperature").build();
    let slider = gtk::Scale::with_range(gtk::Orientation::Horizontal, 1900.0, 5900.0, 100.0);
    slider.set_inverted(true);
    slider.set_value(tinted.unwrap_or(night) as f64);
    slider.set_hexpand(true);
    slider.set_size_request(220, -1);
    slider.set_draw_value(false);
    // Between its ends' names, as in macOS.
    let range = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    range.set_sensitive(tinted.is_some());
    for (text, last) in [("Less Warm", false), ("More Warm", true)] {
        let label = gtk::Label::new(Some(text));
        label.add_css_class("dim-label");
        label.add_css_class("caption");
        if last {
            range.append(&slider);
        }
        range.append(&label);
    }
    let pending = Rc::new(RefCell::new(None));
    slider.connect_value_changed(move |slider| {
        let kelvin = ((slider.value() / 100.0).round() * 100.0) as u32;
        debounce(&pending, 300, move || {
            system::set_night_shift(&kelvin.to_string())
        });
    });
    let tracked = range.clone();
    switch.connect_active_notify(move |switch| {
        let on = switch.is_active();
        tracked.set_sensitive(on);
        system::set_night_shift(if on { "on" } else { "off" });
    });
    row.add_suffix(&range);
    group.add(&switch);
    group.add(&row);
    group
}
