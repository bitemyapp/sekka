// SPDX-License-Identifier: MIT OR Apache-2.0
//! The Sound window, after Aizome's Sound settings, on one page: the
//! outputs and inputs to choose from, the chosen one's volume, balance and
//! port, the input's level, each application's volume and device, and the
//! sound cards' profiles.
//!
//! Reads and changes run on worker threads through pactl (audio.rs), one at
//! a time: changes in the order made, a slider's gesture coalesced to its
//! latest value, and a reading never shown over a change made since it
//! started. The window reads again on each change the audio service
//! reports, and every few seconds. A section's rows are rebuilt only when
//! what it lists changes; volumes, mute, the defaults, routing and profiles
//! are brought in line in place, so a reading doesn't take a control from
//! under the pointer.
use crate::{
    audio::{self, Action, Level},
    meter::{self, Meter},
    model::{self, Card, Kind, Node, Profile, Snapshot},
};
use adw::prelude::*;
use gtk::glib;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    time::{Duration, Instant},
};

/// How often the window reads the audio service, besides each change it
/// reports.
const READ_SECONDS: u32 = 4;
/// How often the input level redraws.
const METER_EVERY: Duration = Duration::from_millis(80);
/// The input level's segments.
const SEGMENTS: f64 = 24.0;
/// How soon a level meter whose connection ended is started again.
const METER_RETRY: Duration = Duration::from_secs(10);

/// Brings a section's widgets in line with a reading.
type Binding = Box<dyn Fn(&Snapshot)>;

/// A group whose rows are replaced only when what they show changes, and
/// otherwise brought in line with each reading by their bindings.
struct Section {
    group: adw::PreferencesGroup,
    rows: RefCell<Vec<gtk::Widget>>,
    bindings: RefCell<Vec<Binding>>,
    key: RefCell<Option<String>>,
}

impl Section {
    fn new(title: &str, description: Option<&str>) -> Section {
        let group = adw::PreferencesGroup::new();
        group.set_title(title);
        group.set_description(description);
        group.set_visible(false);
        Section {
            group,
            rows: RefCell::new(Vec::new()),
            bindings: RefCell::new(Vec::new()),
            key: RefCell::new(None),
        }
    }

    /// Show these rows, rebuilt only when `key` (what they show) changed.
    fn show(&self, key: String, rows: impl FnOnce(&mut Vec<Binding>) -> Vec<gtk::Widget>) {
        self.group.set_visible(true);
        if self.key.borrow().as_deref() == Some(key.as_str()) {
            return;
        }
        self.clear();
        let mut bindings = Vec::new();
        let rows = rows(&mut bindings);
        for row in &rows {
            self.group.add(row);
        }
        *self.rows.borrow_mut() = rows;
        *self.bindings.borrow_mut() = bindings;
        *self.key.borrow_mut() = Some(key);
    }

    /// Hide it, its rows gone.
    fn hide(&self) {
        self.clear();
        self.group.set_visible(false);
    }

    fn clear(&self) {
        for row in self.rows.borrow_mut().drain(..) {
            self.group.remove(&row);
        }
        self.bindings.borrow_mut().clear();
        self.key.borrow_mut().take();
    }

    fn update(&self, s: &Snapshot) {
        for binding in self.bindings.borrow().iter() {
            binding(s);
        }
    }
}

struct Ui {
    window: adw::ApplicationWindow,
    toasts: adw::ToastOverlay,
    banner: adw::Banner,
    content: gtk::Box,
    outputs: Section,
    output: Section,
    inputs: Section,
    input: Section,
    apps: Section,
    cards: Section,
    /// The reading shown.
    snapshot: RefCell<Snapshot>,
    /// Set while widgets are brought in line with a reading: their handlers
    /// then change nothing.
    updating: Cell<bool>,
    /// A reading or a change runs.
    busy: Cell<bool>,
    /// Read when nothing else runs: something changed since the last
    /// reading began.
    stale: Cell<bool>,
    pending: RefCell<VecDeque<Action>>,
    presented: Cell<bool>,
    /// The level meter, the input it follows and when it started; the
    /// input level's bar, and what it shows.
    meter: RefCell<Option<(String, Meter, Instant)>>,
    level: RefCell<Option<gtk::LevelBar>>,
    shown_level: Cell<f64>,
}

impl Ui {
    fn sections(&self) -> [&Section; 6] {
        [
            &self.outputs,
            &self.output,
            &self.inputs,
            &self.input,
            &self.apps,
            &self.cards,
        ]
    }
}

thread_local! {
    static UI: RefCell<Option<Rc<Ui>>> = const { RefCell::new(None) };
}

/// The window, made the first time; shown as soon as it has something to
/// show.
pub fn open(app: &adw::Application) {
    if let Some(ui) = UI.with_borrow(Clone::clone) {
        ui.window.present();
        return;
    }
    let ui = build(app);
    UI.with_borrow_mut(|slot| *slot = Some(ui.clone()));
    pump(&ui);
    // A reading takes a few hundredths of a second; show the window then,
    // or now if it is slower, rather than empty.
    let soon = ui.clone();
    glib::timeout_add_local_once(Duration::from_millis(250), move || present(&soon));
}

fn present(ui: &Ui) {
    if !ui.presented.replace(true) {
        ui.window.present();
    }
}

/// Run `work` on a worker thread, then `then` with its result on the
/// window's.
fn background<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    then: impl FnOnce(T) + 'static,
) {
    let (sender, receiver) = async_channel::bounded(1);
    std::thread::spawn(move || {
        let _ = sender.send_blocking(work());
    });
    glib::spawn_future_local(async move {
        if let Ok(result) = receiver.recv().await {
            then(result);
        }
    });
}

fn toast(ui: &Ui, text: &str) {
    let toast = adw::Toast::new(text);
    toast.set_use_markup(false);
    ui.toasts.add_toast(toast);
}

fn build(app: &adw::Application) -> Rc<Ui> {
    let window = adw::ApplicationWindow::builder()
        .application(app)
        .title("Sound")
        .icon_name("audio-speakers-symbolic")
        .default_width(720)
        .default_height(740)
        .build();
    let outputs = Section::new("Output", None);
    let output = Section::new("", None);
    let inputs = Section::new("Input", None);
    let input = Section::new("", None);
    let apps = Section::new(
        "Applications",
        Some("Each application's own volume and device, while it plays or records."),
    );
    let cards = Section::new(
        "Device profiles",
        Some(
            "How each sound device is used. A Bluetooth headset's microphone works only in \
             headset mode, usually with lower-quality playback. Off turns a device off.",
        ),
    );
    let content = gtk::Box::new(gtk::Orientation::Vertical, 24);
    content.set_margin_top(18);
    content.set_margin_bottom(24);
    content.set_margin_start(18);
    content.set_margin_end(18);
    // The devices, and the chosen one's controls just under them.
    for (devices, controls) in [(&outputs, &output), (&inputs, &input)] {
        let pair = gtk::Box::new(gtk::Orientation::Vertical, 12);
        pair.append(&devices.group);
        pair.append(&controls.group);
        content.append(&pair);
    }
    content.append(&apps.group);
    content.append(&cards.group);
    let clamp = adw::Clamp::builder()
        .maximum_size(640)
        .child(&content)
        .build();
    let scroller = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&clamp)
        .build();
    let banner = adw::Banner::builder()
        .title("Connecting to the audio service…")
        .use_markup(false)
        .revealed(true)
        .build();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 0);
    body.append(&banner);
    body.append(&scroller);
    let view = adw::ToolbarView::new();
    view.add_top_bar(&adw::HeaderBar::new());
    view.set_content(Some(&body));
    let toasts = adw::ToastOverlay::new();
    toasts.set_child(Some(&view));
    window.set_content(Some(&toasts));

    let ui = Rc::new(Ui {
        window: window.clone(),
        toasts,
        banner,
        content,
        outputs,
        output,
        inputs,
        input,
        apps,
        cards,
        snapshot: RefCell::new(Snapshot::default()),
        updating: Cell::new(false),
        busy: Cell::new(false),
        stale: Cell::new(true),
        pending: RefCell::new(VecDeque::new()),
        presented: Cell::new(false),
        meter: RefCell::new(None),
        level: RefCell::new(None),
        shown_level: Cell::new(0.0),
    });

    // Escape closes the window, as it does Tatami's other floating windows.
    let shortcuts = gtk::ShortcutController::new();
    let escape = Rc::downgrade(&ui);
    shortcuts.add_shortcut(gtk::Shortcut::new(
        gtk::ShortcutTrigger::parse_string("Escape"),
        Some(gtk::CallbackAction::new(move |_, _| {
            if let Some(ui) = escape.upgrade() {
                ui.window.close();
            }
            glib::Propagation::Stop
        })),
    ));
    window.add_controller(shortcuts);
    // The meter's stream ends with the window.
    let closed = Rc::downgrade(&ui);
    window.connect_close_request(move |_| {
        if let Some(ui) = closed.upgrade() {
            ui.meter.borrow_mut().take();
        }
        glib::Propagation::Proceed
    });

    // Read again on each change the audio service reports, a moment after
    // the first of a burst.
    let (sender, receiver) = async_channel::unbounded::<()>();
    audio::monitor(move || {
        let _ = sender.send_blocking(());
    });
    let changed = Rc::downgrade(&ui);
    glib::spawn_future_local(async move {
        while receiver.recv().await.is_ok() {
            glib::timeout_future(Duration::from_millis(150)).await;
            while receiver.try_recv().is_ok() {}
            match changed.upgrade() {
                Some(ui) => {
                    ui.stale.set(true);
                    pump(&ui);
                }
                None => break,
            }
        }
    });
    let timer = Rc::downgrade(&ui);
    glib::timeout_add_seconds_local(READ_SECONDS, move || match timer.upgrade() {
        Some(ui) => {
            ui.stale.set(true);
            pump(&ui);
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });
    let meter = Rc::downgrade(&ui);
    glib::timeout_add_local(METER_EVERY, move || match meter.upgrade() {
        Some(ui) => {
            show_level(&ui);
            glib::ControlFlow::Continue
        }
        None => glib::ControlFlow::Break,
    });
    ui
}

/// Send a change, after those before it; a slider's earlier values not yet
/// sent are dropped for its latest.
fn queue(ui: &Rc<Ui>, action: Action) {
    if ui.updating.get() {
        return;
    }
    {
        let mut pending = ui.pending.borrow_mut();
        pending.retain(|earlier| !action.replaces(earlier));
        pending.push_back(action);
    }
    pump(ui);
}

/// Send the next change, or else read if something changed; one at a time.
fn pump(ui: &Rc<Ui>) {
    if ui.busy.get() {
        return;
    }
    let action = ui.pending.borrow_mut().pop_front();
    if action.is_none() && !ui.stale.get() {
        return;
    }
    ui.busy.set(true);
    let ui = ui.clone();
    if let Some(action) = action {
        background(
            move || audio::apply(&action),
            move |result| {
                ui.busy.set(false);
                if let Err(error) = result {
                    toast(&ui, &error);
                }
                // Read what it did once no other change waits.
                ui.stale.set(true);
                pump(&ui);
            },
        );
        return;
    }
    ui.stale.set(false);
    background(audio::snapshot, move |state| {
        ui.busy.set(false);
        match state {
            Ok(state) => {
                ui.banner.set_revealed(false);
                ui.content.set_sensitive(true);
                // A change made while it read would be undone by showing it;
                // the reading after that change shows it instead.
                if ui.pending.borrow().is_empty() {
                    render(&ui, &state);
                    *ui.snapshot.borrow_mut() = state;
                }
            }
            Err(error) => {
                ui.pending.borrow_mut().clear();
                ui.banner.set_title(&error);
                ui.banner.set_revealed(true);
                ui.content.set_sensitive(false);
            }
        }
        present(&ui);
        pump(&ui);
    });
}

fn render(ui: &Rc<Ui>, s: &Snapshot) {
    ui.updating.set(true);
    for kind in [Kind::Output, Kind::Input] {
        devices(ui, s, kind);
        controls(ui, s, kind);
    }
    applications(ui, s);
    cards(ui, s);
    for section in ui.sections() {
        section.update(s);
    }
    ui.updating.set(false);
    follow_input(ui, s);
}

/// The outputs or inputs to choose from, the default checked.
fn devices(ui: &Rc<Ui>, s: &Snapshot, kind: Kind) {
    let section = if kind == Kind::Output {
        &ui.outputs
    } else {
        &ui.inputs
    };
    let rows: Vec<(u32, String, String, &str)> = s
        .devices(kind)
        .map(|d| {
            (
                d.id,
                d.name.clone(),
                model::device_name(d, &s.nodes),
                model::glyph(d),
            )
        })
        .collect();
    section.show(format!("{rows:?}"), |bindings| {
        if rows.is_empty() {
            let row = adw::ActionRow::builder()
                .title(if kind == Kind::Output {
                    "No output devices"
                } else {
                    "No input devices"
                })
                .subtitle("Connected audio devices appear here.")
                .build();
            return vec![row.upcast()];
        }
        rows.iter()
            .map(|(id, name, title, glyph)| {
                device_row(ui, kind, *id, name, title, glyph, bindings).upcast()
            })
            .collect()
    });
}

fn device_row(
    ui: &Rc<Ui>,
    kind: Kind,
    id: u32,
    name: &str,
    title: &str,
    glyph: &str,
    bindings: &mut Vec<Binding>,
) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(title)
        .use_markup(false)
        .activatable(true)
        .build();
    let tile = tile(glyph);
    row.add_prefix(&tile);
    let check = gtk::Image::from_icon_name("object-select-symbolic");
    check.add_css_class("accent");
    row.add_suffix(&check);
    let weak = Rc::downgrade(ui);
    let chosen = name.to_owned();
    row.connect_activated(move |_| {
        if let Some(ui) = weak.upgrade()
            && ui.snapshot.borrow().default_name(kind) != chosen
        {
            queue(&ui, Action::Default(kind, chosen.clone()));
        }
    });
    let (row_, title) = (row.clone(), title.to_owned());
    bindings.push(Box::new(move |s| {
        let Some(device) = s.node(kind, id) else {
            return;
        };
        let current = s.default_name(kind) == device.name;
        check.set_visible(current);
        set_class(&row_, "current", current);
        set_class(&tile, "current", current);
        row_.set_subtitle(&model::device_detail(device, s, &title));
        // What assistive technology hears for the check.
        row_.upcast_ref::<gtk::Widget>()
            .update_state(&[gtk::accessible::State::Selected(Some(current))]);
    }));
    row
}

/// What a device's or stream's volume and balance controls send together:
/// the latest of each.
struct Levels {
    volume: Cell<f64>,
    balance: Cell<f64>,
}

/// The default output's or input's volume, balance and port, and the
/// input's level.
fn controls(ui: &Rc<Ui>, s: &Snapshot, kind: Kind) {
    let section = if kind == Kind::Output {
        &ui.output
    } else {
        &ui.input
    };
    let Some(device) = s.current(kind) else {
        section.hide();
        if kind == Kind::Input {
            ui.level.borrow_mut().take();
        }
        return;
    };
    let ports: Vec<(String, String)> = device
        .usable_ports()
        .iter()
        .map(|port| (port.name.clone(), port.label.clone()))
        .collect();
    let key = format!("{} {} {:?} {ports:?}", device.id, device.name, device.sides);
    section.show(key, |bindings| {
        let levels = Rc::new(Levels {
            volume: Cell::new(device.volume),
            balance: Cell::new(device.balance),
        });
        let (title, mute, balance) = if kind == Kind::Output {
            ("Output volume", "Mute output", "Output balance")
        } else {
            ("Input volume", "Mute input", "Input balance")
        };
        let row = adw::ActionRow::builder()
            .title(title)
            .use_markup(false)
            .build();
        for widget in volume(ui, device, title, mute, 220, true, &levels, bindings) {
            row.add_suffix(&widget);
        }
        let mut rows: Vec<gtk::Widget> = vec![row.upcast()];
        if device.stereo() {
            rows.push(balance_row(ui, device, balance, &levels, bindings).upcast());
        }
        if ports.len() > 1 {
            rows.push(port_row(ui, device, &ports, bindings).upcast());
        }
        if kind == Kind::Input {
            let (row, bar) = level_row();
            *ui.level.borrow_mut() = Some(bar);
            rows.push(row.upcast());
        }
        rows
    });
}

/// A mute button and a volume slider for `node`, and for a device the
/// volume in words after them.
#[allow(clippy::too_many_arguments)]
fn volume(
    ui: &Rc<Ui>,
    node: &Node,
    title: &str,
    mute_title: &str,
    width: i32,
    words: bool,
    levels: &Rc<Levels>,
    bindings: &mut Vec<Binding>,
) -> Vec<gtk::Widget> {
    let (kind, id) = (node.kind, node.id);
    let mute = gtk::ToggleButton::builder()
        .valign(gtk::Align::Center)
        .css_classes(["flat"])
        .build();
    mute.update_property(&[gtk::accessible::Property::Label(mute_title)]);
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.set_draw_value(false);
    scale.set_width_request(width);
    scale.set_valign(gtk::Align::Center);
    scale.update_property(&[gtk::accessible::Property::Label(title)]);
    let label = words.then(|| {
        let label = gtk::Label::new(None);
        label.set_width_chars(6);
        label.set_xalign(1.0);
        label.add_css_class("dim-label");
        label.add_css_class("numeric");
        label
    });

    let weak = Rc::downgrade(ui);
    let (sides, to_levels, to_label) = (node.sides.clone(), levels.clone(), label.clone());
    scale.connect_value_changed(move |scale| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.updating.get() {
            return;
        }
        to_levels.volume.set(scale.value());
        if let Some(label) = &to_label {
            label.set_text(&percent(scale.value()));
        }
        let level = Level {
            volume: scale.value(),
            balance: to_levels.balance.get(),
            sides: sides.clone(),
        };
        queue(&ui, Action::Volume(kind, id, level));
    });
    let weak = Rc::downgrade(ui);
    let (to_scale, to_label, to_levels) = (
        scale.downgrade(),
        label.as_ref().map(|l| l.downgrade()),
        levels.clone(),
    );
    mute.connect_toggled(move |mute| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.updating.get() {
            return;
        }
        if let Some(scale) = to_scale.upgrade() {
            let label = to_label.as_ref().and_then(|l| l.upgrade());
            let writable = ui
                .snapshot
                .borrow()
                .node(kind, id)
                .is_none_or(|n| n.writable);
            show_volume(
                kind,
                mute,
                &scale,
                label.as_ref(),
                mute.is_active(),
                to_levels.volume.get(),
                writable,
            );
        }
        queue(&ui, Action::Mute(kind, id, mute.is_active()));
    });

    let mut widgets: Vec<gtk::Widget> = vec![mute.clone().upcast(), scale.clone().upcast()];
    widgets.extend(label.clone().map(|label| label.upcast()));
    let levels = levels.clone();
    bindings.push(Box::new(move |s| {
        let Some(n) = s.node(kind, id) else {
            return;
        };
        levels.volume.set(n.volume);
        levels.balance.set(n.balance);
        // Louder than 100% only where something else made it so.
        let max = if n.volume > 100.5 {
            model::MAX_VOLUME
        } else {
            100.0
        };
        if scale.adjustment().upper() != max {
            scale.set_range(0.0, max);
            scale.clear_marks();
            if max > 100.0 {
                scale.add_mark(100.0, gtk::PositionType::Bottom, None);
            }
        }
        scale.set_value(n.volume.min(max));
        mute.set_active(n.muted);
        show_volume(
            kind,
            &mute,
            &scale,
            label.as_ref(),
            n.muted,
            n.volume,
            n.writable,
        );
    }));
    widgets
}

/// The mute button's glyph and the slider and words for a volume.
fn show_volume(
    kind: Kind,
    mute: &gtk::ToggleButton,
    scale: &gtk::Scale,
    label: Option<&gtk::Label>,
    muted: bool,
    volume: f64,
    writable: bool,
) {
    let glyph = match kind.devices() {
        Kind::Input if muted => "microphone-disabled-symbolic",
        Kind::Input => "audio-input-microphone-symbolic",
        _ if muted || volume < 1.0 => "audio-volume-muted-symbolic",
        _ if volume < 34.0 => "audio-volume-low-symbolic",
        _ if volume < 67.0 => "audio-volume-medium-symbolic",
        _ if volume <= 100.5 => "audio-volume-high-symbolic",
        _ => "audio-volume-overamplified-symbolic",
    };
    mute.set_icon_name(glyph);
    mute.set_tooltip_text(Some(if muted { "Unmute" } else { "Mute" }));
    scale.set_sensitive(writable && !muted);
    if let Some(label) = label {
        label.set_text(&if muted {
            "Muted".to_owned()
        } else {
            percent(volume)
        });
    }
}

fn balance_row(
    ui: &Rc<Ui>,
    device: &Node,
    title: &str,
    levels: &Rc<Levels>,
    bindings: &mut Vec<Binding>,
) -> adw::ActionRow {
    let (kind, id, sides) = (device.kind, device.id, device.sides.clone());
    let row = adw::ActionRow::builder()
        .title("Balance")
        .use_markup(false)
        .build();
    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, -1.0, 1.0, 0.02);
    scale.set_draw_value(false);
    scale.set_width_request(200);
    scale.set_valign(gtk::Align::Center);
    scale.add_mark(0.0, gtk::PositionType::Bottom, None);
    scale.update_property(&[gtk::accessible::Property::Label(title)]);
    let side = |text: &str| {
        let label = gtk::Label::new(Some(text));
        label.add_css_class("dim-label");
        label
    };
    row.add_suffix(&side("L"));
    row.add_suffix(&scale);
    row.add_suffix(&side("R"));
    let weak = Rc::downgrade(ui);
    let to_levels = levels.clone();
    scale.connect_value_changed(move |scale| {
        let Some(ui) = weak.upgrade() else {
            return;
        };
        if ui.updating.get() {
            return;
        }
        // Near the middle is the middle.
        let balance = if scale.value().abs() < 0.04 {
            0.0
        } else {
            scale.value()
        };
        to_levels.balance.set(balance);
        let level = Level {
            volume: to_levels.volume.get(),
            balance,
            sides: sides.clone(),
        };
        queue(&ui, Action::Volume(kind, id, level));
    });
    bindings.push(Box::new(move |s| {
        if let Some(n) = s.node(kind, id) {
            scale.set_value(n.balance);
            scale.set_sensitive(n.writable);
        }
    }));
    row
}

fn port_row(
    ui: &Rc<Ui>,
    device: &Node,
    ports: &[(String, String)],
    bindings: &mut Vec<Binding>,
) -> adw::ComboRow {
    let (kind, id) = (device.kind, device.id);
    let labels: Vec<&str> = ports.iter().map(|(_, label)| label.as_str()).collect();
    let row = combo("Port", &labels);
    let names: Vec<String> = ports.iter().map(|(name, _)| name.clone()).collect();
    let weak = Rc::downgrade(ui);
    let values = names.clone();
    row.connect_selected_notify(move |row| {
        if let (Some(ui), Some(port)) = (weak.upgrade(), values.get(row.selected() as usize)) {
            queue(&ui, Action::Port(kind, id, port.clone()));
        }
    });
    let row_ = row.clone();
    bindings.push(Box::new(move |s| {
        if let Some(n) = s.node(kind, id) {
            row_.set_selected(position(&names, &n.port));
        }
    }));
    row
}

/// The input's level, as Aizome's: segments that light as it gets louder.
fn level_row() -> (adw::ActionRow, gtk::LevelBar) {
    let row = adw::ActionRow::builder()
        .title("Input level")
        .subtitle("Speak to see the level move")
        .use_markup(false)
        .build();
    let bar = gtk::LevelBar::builder()
        .mode(gtk::LevelBarMode::Discrete)
        .min_value(0.0)
        .max_value(SEGMENTS)
        .width_request(268)
        .valign(gtk::Align::Center)
        .build();
    // The segments' colors are the stylesheet's, by position, rather than
    // the whole bar's by its value.
    for offset in ["low", "high", "full"] {
        bar.remove_offset_value(Some(offset));
    }
    bar.add_css_class("input-level");
    bar.update_property(&[gtk::accessible::Property::Label("Input level")]);
    row.add_suffix(&bar);
    (row, bar)
}

/// Redraw the input level: quick to rise, slower to fall, as a meter's.
fn show_level(ui: &Ui) {
    if !ui.window.is_visible() {
        return;
    }
    let Some(bar) = ui.level.borrow().clone() else {
        return;
    };
    let peak = ui
        .meter
        .borrow()
        .as_ref()
        .map_or(0.0, |(_, meter, _)| meter::loudness(meter.level()) as f64);
    let shown = peak.max(ui.shown_level.get() * 0.85);
    ui.shown_level.set(shown);
    bar.set_value((shown * SEGMENTS).round());
}

/// Keep the level meter on the default input.
fn follow_input(ui: &Ui, s: &Snapshot) {
    let wanted = s.current(Kind::Input).map(|input| input.name.clone());
    let mut meter = ui.meter.borrow_mut();
    let restart = match (meter.as_ref(), wanted.as_ref()) {
        (Some((name, running, started)), Some(wanted)) => {
            name != wanted || (running.ended() && started.elapsed() >= METER_RETRY)
        }
        (None, Some(_)) => true,
        (_, None) => false,
    };
    if wanted.is_none() {
        meter.take();
    } else if restart {
        *meter = wanted.map(|name| {
            let started = Meter::start(&name);
            (name, started, Instant::now())
        });
    }
}

/// The applications playing and recording, each with its volume and
/// device.
fn applications(ui: &Rc<Ui>, s: &Snapshot) {
    let streams: Vec<&Node> = s.streams().collect();
    if streams.is_empty() {
        ui.apps.hide();
        return;
    }
    let choices = |kind| -> Vec<Route> {
        s.devices(kind)
            .map(|d| Route {
                id: d.id,
                name: d.name.clone(),
                label: model::device_name(d, &s.nodes),
                target: d.object.zip(d.serial),
            })
            .collect()
    };
    let (outputs, inputs) = (choices(Kind::Output), choices(Kind::Input));
    let rows: Vec<(Kind, u32, String, &str)> = streams
        .iter()
        .map(|n| (n.kind, n.id, n.label.clone(), model::glyph(n)))
        .collect();
    ui.apps
        .show(format!("{rows:?}{outputs:?}{inputs:?}"), |bindings| {
            streams
                .iter()
                .map(|stream| {
                    let devices = if stream.kind == Kind::Playback {
                        &outputs
                    } else {
                        &inputs
                    };
                    app_row(ui, stream, devices, bindings).upcast()
                })
                .collect()
        });
}

/// A device an application's stream can be moved to.
#[derive(Debug)]
struct Route {
    id: u32,
    name: String,
    label: String,
    /// Its PipeWire node and serial.
    target: Option<(u32, u64)>,
}

fn app_row(
    ui: &Rc<Ui>,
    stream: &Node,
    devices: &[Route],
    bindings: &mut Vec<Binding>,
) -> adw::ActionRow {
    let (kind, id) = (stream.kind, stream.id);
    let row = adw::ActionRow::builder()
        .title(stream.label.as_str())
        .use_markup(false)
        .title_lines(1)
        .subtitle_lines(1)
        .build();
    let tile = tile(model::glyph(stream));
    row.add_prefix(&tile);
    if devices.len() > 1 {
        let labels: Vec<&str> = devices.iter().map(|d| d.label.as_str()).collect();
        let dropdown = gtk::DropDown::from_strings(&labels);
        dropdown.set_valign(gtk::Align::Center);
        // Its name is the device chosen (its button's); this says what for.
        let what = if kind == Kind::Playback {
            "Output"
        } else {
            "Input"
        };
        dropdown.update_property(&[gtk::accessible::Property::Description(&format!(
            "{what} for {}",
            stream.label
        ))]);
        if !stream.movable {
            dropdown.set_sensitive(false);
            dropdown.set_tooltip_text(Some("This application keeps its own device"));
        }
        let ids: Vec<u32> = devices.iter().map(|d| d.id).collect();
        let routes: Vec<(String, Option<(u32, u64)>)> =
            devices.iter().map(|d| (d.name.clone(), d.target)).collect();
        let (weak, object) = (Rc::downgrade(ui), stream.object);
        dropdown.connect_selected_notify(move |dropdown| {
            let (Some(ui), Some(object), Some((name, target))) = (
                weak.upgrade(),
                object,
                routes.get(dropdown.selected() as usize),
            ) else {
                return;
            };
            if ui.updating.get() {
                return;
            }
            // To the default: follow it from now on, as pipewire-pulse's
            // move does.
            let target = if ui.snapshot.borrow().default_name(kind) == name {
                None
            } else if target.is_some() {
                *target
            } else {
                return;
            };
            queue(&ui, Action::Move(object, target));
        });
        let dropdown_ = dropdown.clone();
        bindings.push(Box::new(move |s| {
            if let Some(n) = s.node(kind, id) {
                dropdown_.set_selected(
                    ids.iter()
                        .position(|id| Some(*id) == n.target)
                        .map_or(gtk::INVALID_LIST_POSITION, |i| i as u32),
                );
            }
        }));
        row.add_suffix(&dropdown);
    }
    let levels = Rc::new(Levels {
        volume: Cell::new(stream.volume),
        balance: Cell::new(stream.balance),
    });
    let title = format!("{} volume", stream.label);
    let mute = format!("Mute {}", stream.label);
    for widget in volume(ui, stream, &title, &mute, 120, false, &levels, bindings) {
        row.add_suffix(&widget);
    }
    let row_ = row.clone();
    bindings.push(Box::new(move |s| {
        if let Some(n) = s.node(kind, id) {
            row_.set_subtitle(&model::stream_detail(n));
            set_class(&tile, "playing", !n.corked);
        }
    }));
    row
}

/// Each sound card that offers a choice of profile.
fn cards(ui: &Rc<Ui>, s: &Snapshot) {
    let cards: Vec<(&Card, Vec<&Profile>)> = s
        .cards
        .iter()
        .map(|card| (card, card.usable_profiles()))
        .filter(|(_, profiles)| profiles.len() > 1)
        .collect();
    if cards.is_empty() {
        ui.cards.hide();
        return;
    }
    // What the rows show, but for the profile in use.
    let key = cards
        .iter()
        .map(|(card, profiles)| format!("{} {} {profiles:?}", card.name, card.label))
        .collect::<Vec<_>>()
        .join("\n");
    ui.cards.show(key, |bindings| {
        cards
            .iter()
            .map(|(card, profiles)| card_row(ui, card, profiles, bindings).upcast())
            .collect()
    });
}

fn card_row(
    ui: &Rc<Ui>,
    card: &Card,
    profiles: &[&Profile],
    bindings: &mut Vec<Binding>,
) -> adw::ComboRow {
    let labels: Vec<&str> = profiles.iter().map(|p| p.label.as_str()).collect();
    let row = combo(&card.label, &labels);
    let bus = model::bus_words(&card.bus, &card.form, card.hdmi());
    if !bus.is_empty() {
        row.set_subtitle(bus);
    }
    row.add_prefix(&tile(model::card_glyph(card)));
    let names: Vec<String> = profiles.iter().map(|p| p.name.clone()).collect();
    let weak = Rc::downgrade(ui);
    let (values, target) = (names.clone(), card.name.clone());
    row.connect_selected_notify(move |row| {
        if let (Some(ui), Some(profile)) = (weak.upgrade(), values.get(row.selected() as usize)) {
            queue(&ui, Action::Profile(target.clone(), profile.clone()));
        }
    });
    let (row_, name) = (row.clone(), card.name.clone());
    bindings.push(Box::new(move |s| {
        if let Some(card) = s.cards.iter().find(|card| card.name == name) {
            row_.set_selected(position(&names, &card.active));
        }
    }));
    row
}

fn combo(title: &str, labels: &[&str]) -> adw::ComboRow {
    adw::ComboRow::builder()
        .title(title)
        .use_markup(false)
        .model(&gtk::StringList::new(labels))
        .build()
}

/// A glyph on a tile, as Aizome's rows have them.
fn tile(glyph: &str) -> gtk::Image {
    let image = gtk::Image::from_icon_name(glyph);
    image.add_css_class("tile");
    image.set_valign(gtk::Align::Center);
    image
}

fn set_class(widget: &impl IsA<gtk::Widget>, class: &str, on: bool) {
    if on {
        widget.add_css_class(class);
    } else {
        widget.remove_css_class(class);
    }
}

fn position(names: &[String], name: &str) -> u32 {
    names
        .iter()
        .position(|n| n == name)
        .map_or(gtk::INVALID_LIST_POSITION, |i| i as u32)
}

fn percent(volume: f64) -> String {
    format!("{volume:.0}%")
}
