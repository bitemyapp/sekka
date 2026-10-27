//! Keyboard: layouts (searched from xkeyboard-config's registry, with
//! variants) and the keys that switch them; key repeat with a place to try
//! it; modifier swaps, Caps Lock as Control, Escape or Compose and the
//! Compose key; and the editor for the system's shortcuts (the compositor's `[keys]`).
//! `[input.keyboard]` and `[keys.bindings]`, applied by the compositor.

pub mod keys;
mod modifiers;
mod record;

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use aizome_sys::keyboard::KeyboardData;

use crate::form;
use crate::pane::{Confirm, Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use keys::Chord;

/// A layout or variant to add: its description, layout and variant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub description: String,
    pub layout: String,
    pub variant: Option<String>,
}

/// The registry, read once.
#[derive(Debug)]
pub struct Registry {
    pub data: KeyboardData,
    pub choices: Vec<Choice>,
}

impl Registry {
    pub fn new(data: KeyboardData) -> Registry {
        let choices = data
            .choices()
            .into_iter()
            .filter(|c| !c.extra)
            .map(|c| Choice {
                description: c.description.to_owned(),
                layout: c.layout.to_owned(),
                variant: c.variant.map(str::to_owned),
            })
            .collect();
        Registry { data, choices }
    }

    /// The registry, or a few layouts when xkeyboard-config isn't there.
    pub fn load() -> Registry {
        match KeyboardData::load() {
            Ok(d) => Registry::new(d),
            Err(e) => {
                log::info!("keyboard layouts: {e}");
                Registry::new(KeyboardData::from_xml(FALLBACK).unwrap_or_default())
            }
        }
    }

    pub fn describe(&self, layout: &str, variant: &str) -> String {
        self.data.describe(layout, variant)
    }
}

const FALLBACK: &str = r#"<?xml version="1.0"?><xkbConfigRegistry version="1.1"><modelList/><layoutList>
<layout><configItem><name>us</name><shortDescription>en</shortDescription><description>English (US)</description></configItem>
<variantList><variant><configItem><name>intl</name><description>English (US, intl., with dead keys)</description></configItem></variant>
<variant><configItem><name>dvorak</name><description>English (Dvorak)</description></configItem></variant>
<variant><configItem><name>colemak</name><description>English (Colemak)</description></configItem></variant></variantList></layout>
<layout><configItem><name>gb</name><shortDescription>en</shortDescription><description>English (UK)</description></configItem></layout>
<layout><configItem><name>jp</name><shortDescription>ja</shortDescription><description>Japanese</description></configItem></layout>
<layout><configItem><name>de</name><shortDescription>de</shortDescription><description>German</description></configItem></layout>
<layout><configItem><name>fr</name><shortDescription>fr</shortDescription><description>French</description></configItem></layout>
</layoutList><optionList/></xkbConfigRegistry>"#;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Typing,
    Shortcuts,
}

/// A command of the user's own being given keys.
#[derive(Clone, Debug, Default)]
pub struct CommandSheet {
    pub command: String,
    pub chord: Option<Chord>,
    pub error: Option<String>,
}

#[derive(Default)]
pub struct Keyboard {
    tab: Tab,
    registry: Option<Arc<Registry>>,
    /// The layout picker, while its sheet is open: the query and the row
    /// chosen (in the filtered list).
    picker: Option<(String, Option<usize>)>,
    /// The action whose shortcut is being recorded.
    recording: Option<String>,
    /// The sheet for a command of the user's own, while it's open.
    command: Option<CommandSheet>,
    trial: String,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Tab(Tab),
    Registry(Arc<Registry>),
    AddLayout,
    PickerQuery(String),
    PickerSelect(usize),
    PickerAdd,
    PickerCancel,
    RemoveLayout(usize),
    MoveUp(usize),
    Switch(usize),
    Delay(f64),
    Rate(f64),
    Trial(String),
    Modifiers(usize),
    Caps(usize),
    Compose(usize),
    NumLock(bool),
    Record(String),
    Recorded(Chord),
    RecordCancel,
    RecordClear,
    Rebind(String, String),
    Reset(String),
    RestoreDefaults,
    RestoreConfirmed,
    AddCommand,
    CommandText(String),
    CommandChord(Chord),
    CommandClear,
    CommandCancel,
    CommandSave,
    RemoveCommand(String),
}

/// What Caps Lock can be, and the option that makes it so.
pub const CAPS: [(&str, Option<&str>); 4] = [
    ("Caps Lock", None),
    ("Control", Some("ctrl:nocaps")),
    ("Escape", Some("caps:escape")),
    ("Compose", Some("compose:caps")),
];

/// Options a Caps Lock choice replaces.
fn is_caps_option(o: &str) -> bool {
    o.starts_with("caps:") || o == "ctrl:nocaps" || o == "ctrl:swapcaps" || o == "compose:caps"
}

pub const COMPOSE: [(&str, Option<&str>); 5] = [
    ("None", None),
    ("Right Alt", Some("compose:ralt")),
    ("Right Control", Some("compose:rctrl")),
    ("Menu", Some("compose:menu")),
    ("Right Super", Some("compose:rwin")),
];

pub const SWITCH: [(&str, Option<&str>); 5] = [
    ("Only from the menu bar", None),
    ("⌥⇧", Some("grp:alt_shift_toggle")),
    ("⌃⇧", Some("grp:ctrl_shift_toggle")),
    ("Both Shift keys", Some("grp:shifts_toggle")),
    ("⌃Space", Some("grp:ctrl_space_toggle")),
];

/// The XKB options, split.
pub fn options(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|o| !o.is_empty()).map(str::to_owned).collect()
}

/// `s` without the options `drop` matches, and with `add`.
pub fn replace_options(s: &str, drop: impl Fn(&str) -> bool, add: Option<&str>) -> String {
    let mut v: Vec<String> = options(s).into_iter().filter(|o| !drop(o)).collect();
    if let Some(a) = add {
        v.push(a.to_owned());
    }
    v.join(",")
}

/// The layouts and their variants, paired.
pub fn layouts(k: &aizome_config::Keyboard) -> Vec<(String, String)> {
    let variants: Vec<&str> = k.variant.split(',').collect();
    k.layouts
        .iter()
        .enumerate()
        .map(|(i, l)| (l.clone(), variants.get(i).map(|v| v.trim().to_owned()).unwrap_or_default()))
        .collect()
}

/// Layouts and variants back into the settings.
pub fn set_layouts(k: &mut aizome_config::Keyboard, list: &[(String, String)]) {
    k.layouts = list.iter().map(|(l, _)| l.clone()).collect();
    k.variant = if list.iter().all(|(_, v)| v.is_empty()) {
        String::new()
    } else {
        list.iter().map(|(_, v)| v.as_str()).collect::<Vec<_>>().join(",")
    };
}

/// The choices matching `query` (every word in the description or name).
pub fn filter<'a>(choices: &'a [Choice], query: &str) -> Vec<&'a Choice> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    choices
        .iter()
        .filter(|c| {
            let hay = format!("{} {} {}", c.description, c.layout, c.variant.as_deref().unwrap_or("")).to_lowercase();
            words.iter().all(|w| hay.contains(w.as_str()))
        })
        .collect()
}

impl Keyboard {
    fn registry(&self) -> Option<&Registry> {
        self.registry.as_deref()
    }

    fn describe(&self, l: &str, v: &str) -> String {
        self.registry()
            .map_or_else(|| if v.is_empty() { l.to_owned() } else { format!("{l} ({v})") }, |r| r.describe(l, v))
    }

    fn typing(&self, vcx: &ViewCx) -> Vec<Element<Msg>> {
        let k = &vcx.settings().input.keyboard;
        let list = layouts(k);
        let n = list.len();
        let mut rows: Vec<form::Row<Msg>> = list
            .iter()
            .enumerate()
            .map(|(i, (l, v))| {
                let short = l.to_uppercase();
                let badge_el = container(text(short).mono().size(10.5).weight(650.0))
                    .padding([2.0, 5.0])
                    .fill(Tone::Sunk)
                    .border(Edge::Line2)
                    .min_width(30.0)
                    .align_x(Align::Center);
                let mut controls = row![].spacing(4.0).align(Align::Center);
                if i > 0 {
                    controls =
                        controls.push(icon_button(Icon::ArrowUp).toolbar().on_press(Msg::MoveUp(i)).tooltip("Move up"));
                }
                controls = controls.push(
                    icon_button(Icon::Minus).toolbar().enabled(n > 1).on_press(Msg::RemoveLayout(i)).tooltip("Remove"),
                );
                let mut r = form::row(self.describe(l, v), controls).leading(badge_el);
                if i == 0 {
                    r = r.detail("First: what the keyboard starts in");
                }
                r
            })
            .collect();
        if !rows.is_empty() {
            let first = rows.remove(0);
            rows.insert(0, first.entry("layouts"));
        }
        rows.push(form::whole(
            container(row![button("Add a Layout…").icon(Icon::Plus).on_press(Msg::AddLayout), spacer()].width(Fill))
                .padding([8.0, form::INSET])
                .width(Fill),
        ));
        let opts = options(&k.options);
        let has = |o: &str| opts.iter().any(|x| x == o);
        let switch_i = SWITCH.iter().position(|(_, o)| o.is_some_and(has)).unwrap_or(0);
        rows.push(
            form::row(
                "Switch layouts with",
                dropdown(SWITCH.map(|s| s.0.to_owned()), switch_i)
                    .on_select(Msg::Switch)
                    .enabled(n > 1)
                    .min_width(160.0),
            )
            .entry("switch-layout")
            .help(if n > 1 {
                "The menu bar shows the layout in use, and switches it too."
            } else {
                "With more than one layout: the keys that switch between them."
            }),
        );

        let caps_i = CAPS.iter().position(|(_, o)| o.is_some_and(has)).unwrap_or(0);
        let compose_i = COMPOSE.iter().position(|(_, o)| o.is_some_and(has)).unwrap_or(0);
        let modifiers_i = modifiers::selected(&k.options);
        let mut modifier_labels: Vec<String> = modifiers::CHOICES.iter().map(|(label, _)| (*label).into()).collect();
        if modifiers_i == modifiers::CHOICES.len() {
            modifier_labels.push("Custom".into());
        }
        let delay = f64::from(k.repeat_delay);
        let rate = f64::from(k.repeat_rate);
        vec![
            vcx.section("Layouts", rows),
            vcx.section(
                "Key Repeat",
                vec![
                    form::row(
                        "Delay until repeat",
                        crate::panes::mouse::speed_slider(-delay, -1000.0..=-150.0, "Long", "Short", |v| {
                            Msg::Delay(-v)
                        }),
                    )
                    .entry("repeat-delay")
                    .detail(format!("{} ms", k.repeat_delay)),
                    form::row(
                        "Repeat rate",
                        crate::panes::mouse::speed_slider(rate, 5.0..=60.0, "Slow", "Fast", Msg::Rate),
                    )
                    .entry("repeat-rate")
                    .detail(format!("{} a second", k.repeat_rate)),
                    form::row(
                        "Try it",
                        text_field(&self.trial).placeholder("Hold a key down here").on_change(Msg::Trial).width(240.0),
                    ),
                ],
            ),
            vcx.section(
                "Keys",
                vec![
                    form::row(
                        "Modifier keys",
                        dropdown(modifier_labels, modifiers_i).on_select(Msg::Modifiers).min_width(240.0),
                    )
                    .entry("modifiers")
                    .help(
                        "Super (❖) is the Windows or Command key. Swap Alt and Super for a Mac-style position. \
                         Applies to all keyboards immediately; choose Default to undo. \
                         Use a left-only swap to keep Right Alt for accents.",
                    ),
                    form::row("Caps Lock key", form::choice(&CAPS.map(|c| c.0), caps_i, Msg::Caps))
                        .entry("caps-lock")
                        .help(match caps_i {
                            1 => "Caps Lock is another Control key, where it's easier to reach.",
                            2 => "Caps Lock is Escape: handy in editors.",
                            3 => "Caps Lock starts a compose sequence: Compose, then ' and e for é.",
                            _ => "Caps Lock locks capitals, as it says.",
                        }),
                    form::row("Compose key", form::choice(&COMPOSE.map(|c| c.0), compose_i, Msg::Compose))
                        .entry("compose")
                        .help(
                            "Press it, then two keys, to type what the keyboard lacks: o and \" for ö, < and = for ≤.",
                        ),
                    form::row("Num Lock on at start", toggle(k.numlock).on_toggle(Msg::NumLock)).entry("numlock"),
                ],
            ),
        ]
    }

    fn shortcuts(&self, vcx: &ViewCx) -> Vec<Element<Msg>> {
        let s = vcx.settings();
        let map = keys::effective(&s.keys.bindings);
        let defaults = keys::effective(&BTreeMap::new());
        let desks = s.desks_or_default().len();
        let mut out = vec![vcx.anchor(
            "shortcuts",
            form::note::<Msg>("Only these belong to the system; every other ❖ shortcut is the app's, as ⌘ is on a Mac. Click a shortcut to type a new one."),
        )];
        for sec in keys::sections(desks) {
            let rows: Vec<form::Row<Msg>> = sec
                .actions
                .iter()
                .map(|(action, label)| {
                    let chords = keys::chords_for(&map, action);
                    let changed = chords != keys::chords_for(&defaults, action);
                    let control: Element<Msg> = if self.recording.as_deref() == Some(action.as_str()) {
                        row![
                            record::recorder(Msg::Recorded, Msg::RecordCancel, Msg::RecordClear).id("recorder"),
                            button("Cancel").compact().on_press(Msg::RecordCancel),
                        ]
                        .spacing(6.0)
                        .align(Align::Center)
                        .into()
                    } else {
                        let mut caps = row![].spacing(3.0).align(Align::Center);
                        if chords.is_empty() {
                            caps = caps.push(text("None").role(Role::Text3));
                        }
                        for (i, c) in chords.iter().enumerate() {
                            if i > 0 {
                                caps = caps.push(text("or").size(11.0).role(Role::Text3));
                            }
                            for g in c.glyphs() {
                                caps = caps.push(keycap(g));
                            }
                        }
                        let mut r = row![
                            crate::widgets::press(caps)
                                .style(crate::widgets::PressStyle::Plain)
                                .padding([3.0, 4.0])
                                .on_press(Msg::Record(action.clone()))
                                .named(format!("Change {label}"))
                        ]
                        .spacing(4.0)
                        .align(Align::Center);
                        if changed {
                            r = r.push(
                                icon_button(Icon::RotateCcw)
                                    .toolbar()
                                    .on_press(Msg::Reset(action.clone()))
                                    .tooltip("Back to the default"),
                            );
                        }
                        r.into()
                    };
                    form::row(label.clone(), control).entry(format!("shortcut-{action}")).compact()
                })
                .collect();
            out.push(vcx.section(sec.title, rows));
        }
        let custom: Vec<(Chord, String)> =
            map.iter().filter(|(_, a)| a.starts_with("spawn ")).map(|(c, a)| (c.clone(), a.clone())).collect();
        let mut rows: Vec<form::Row<Msg>> = custom
            .into_iter()
            .map(|(c, a)| {
                let mut caps = row![].spacing(3.0).align(Align::Center);
                for g in c.glyphs() {
                    caps = caps.push(keycap(g));
                }
                caps = caps.push(space(6.0)).push(
                    icon_button(Icon::Minus).toolbar().on_press(Msg::RemoveCommand(c.to_config())).tooltip("Remove"),
                );
                form::row(keys::command_label(a.trim_start_matches("spawn ")), caps)
                    .leading(icon(Icon::SquareTerminal).size(16.0).role(Role::Text3))
                    .compact()
            })
            .collect();
        rows.push(
            form::whole(
                container(
                    row![button("Add a Command…").icon(Icon::Plus).on_press(Msg::AddCommand), spacer()].width(Fill),
                )
                .padding([8.0, form::INSET])
                .width(Fill),
            )
            .entry("commands"),
        );
        out.push(vcx.section_with(
            "Your Commands",
            rows,
            Some("Programs of your own on keys of their own: a terminal on ❖⌥T, a screenshot tool, a script.".into()),
        ));
        out.push(
            row![
                spacer(),
                button("Restore Defaults").enabled(!s.keys.bindings.is_empty()).on_press(Msg::RestoreDefaults)
            ]
            .width(Fill)
            .into(),
        );
        out
    }

    fn command_view(&self, c: &CommandSheet) -> Element<Msg> {
        let mut caps = row![].spacing(3.0).align(Align::Center);
        match &c.chord {
            Some(ch) => {
                for g in ch.glyphs() {
                    caps = caps.push(keycap(g));
                }
            }
            None => caps = caps.push(text("No keys yet").role(Role::Text3)),
        }
        column![
            text("A Command on Keys of Its Own").size(15.0).weight(650.0),
            form::note("What to run, as you would type it in a terminal, and the keys that run it."),
            text_field(&c.command)
                .mono()
                .placeholder("foot -e htop")
                .on_change(Msg::CommandText)
                .id("command-text")
                .width(Fill),
            row![record::recorder(Msg::CommandChord, Msg::CommandCancel, Msg::CommandClear).id("command-keys"), caps]
                .spacing(10.0)
                .align(Align::Center),
            text(c.error.clone().unwrap_or_default()).size(11.0).role(Role::Danger),
            row![
                spacer(),
                button("Cancel").cancel().on_press(Msg::CommandCancel),
                button("Add")
                    .primary()
                    .default()
                    .enabled(!c.command.trim().is_empty() && c.chord.is_some())
                    .on_press(Msg::CommandSave),
            ]
            .spacing(8.0)
            .width(Fill),
        ]
        .spacing(10.0)
        .width(420.0)
        .into()
    }

    fn picker_view(&self) -> Element<Msg> {
        let (query, selected) = self.picker.clone().unwrap_or_default();
        let choices: Rc<Vec<Choice>> = Rc::new(
            self.registry().map(|r| filter(&r.choices, &query).into_iter().cloned().collect()).unwrap_or_default(),
        );
        let count = choices.len();
        let c2 = choices.clone();
        let list = virtual_list(count, 26.0, move |i| {
            let c = &c2[i];
            row![
                text(c.description.clone()).width(Fill),
                text(match &c.variant {
                    Some(v) => format!("{} {v}", c.layout),
                    None => c.layout.clone(),
                })
                .mono()
                .size(11.0)
                .role(Role::Text3),
            ]
            .spacing(8.0)
            .padding([0.0, 8.0])
            .align(Align::Center)
        })
        .selected(selected)
        .on_select(|v: Vec<usize>| Msg::PickerSelect(v.first().copied().unwrap_or(0)))
        .on_activate(|i| {
            let _ = i;
            Msg::PickerAdd
        })
        .height(300.0)
        .width(Fill);
        column![
            text("Add a Keyboard Layout").size(15.0).weight(650.0),
            search_field(&query)
                .placeholder("Language, country or layout")
                .on_change(Msg::PickerQuery)
                .id("layout-search")
                .width(Fill),
            container(list).border(Edge::Line).width(Fill),
            text(format!("{count} layouts")).size(11.0).role(Role::Text3),
            row![
                spacer(),
                button("Cancel").cancel().on_press(Msg::PickerCancel),
                button("Add").primary().default().enabled(selected.is_some()).on_press(Msg::PickerAdd)
            ]
            .spacing(8.0)
            .width(Fill),
        ]
        .spacing(12.0)
        .width(420.0)
        .into()
    }
}

impl Pane for Keyboard {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "keyboard"
    }

    fn title(&self) -> &'static str {
        "Keyboard"
    }

    fn icon(&self) -> Icon {
        Icon::Keyboard
    }

    fn tile(&self) -> Tile {
        Tile::Slate
    }

    fn group(&self) -> Group {
        Group::Devices
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["typing", "input", "xkb", "keys"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        let mut v = vec![
            SettingEntry::new("layouts", "Keyboard layouts").keywords(&[
                "input sources",
                "language",
                "dvorak",
                "colemak",
                "us",
                "jp",
            ]),
            SettingEntry::new("switch-layout", "Switch layouts with").keywords(&["input source", "toggle", "grp"]),
            SettingEntry::new("repeat-delay", "Delay until repeat").keywords(&["key repeat", "hold"]),
            SettingEntry::new("repeat-rate", "Repeat rate").keywords(&["key repeat", "speed"]),
            SettingEntry::new("modifiers", "Modifier keys").keywords(&[
                "swap", "remap", "super", "alt", "control", "ctrl", "windows", "command", "option", "mac", "macOS",
            ]),
            SettingEntry::new("caps-lock", "Caps Lock key").keywords(&[
                "remap caps lock",
                "control",
                "escape",
                "ctrl:nocaps",
            ]),
            SettingEntry::new("compose", "Compose key").keywords(&["accents", "special characters", "multi key"]),
            SettingEntry::new("numlock", "Num Lock on at start").keywords(&["numpad", "keypad"]),
            SettingEntry::new("shortcuts", "Keyboard shortcuts").keywords(&["hotkeys", "bindings", "keys"]),
            SettingEntry::new("commands", "Commands on keys of your own").keywords(&[
                "custom shortcut",
                "launch",
                "run a program",
                "spawn",
            ]),
        ];
        for s in keys::sections(4) {
            for (a, label) in s.actions {
                if a.starts_with("take-to-desk-") || (a.starts_with("go-to-desk-") && a != "go-to-desk-1") {
                    continue;
                }
                v.push(SettingEntry::new(format!("shortcut-{a}"), label).keywords(&["shortcut", "keys"]));
            }
        }
        v
    }

    fn shown(&mut self, cx: &mut PaneCx<Msg>) {
        if self.registry.is_none() {
            if cx.sample() {
                // The same every time, for the tests' pictures.
                self.registry = Some(Arc::new(Registry::load()));
            } else {
                cx.spawn(|| Msg::Registry(Arc::new(Registry::load())));
            }
        }
    }

    fn hidden(&mut self, _cx: &mut PaneCx<Msg>) {
        self.recording = None;
    }

    fn focus_entry(&mut self, entry: &str, _cx: &mut PaneCx<Msg>) {
        self.tab = if entry.starts_with("shortcut") || entry == "commands" { Tab::Shortcuts } else { Tab::Typing };
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::Tab(t) => {
                self.tab = t;
                self.recording = None;
            }
            Msg::Registry(r) => self.registry = Some(r),
            Msg::AddLayout => {
                self.picker = Some((String::new(), None));
                cx.open_sheet();
                cx.focus("layout-search");
            }
            Msg::PickerQuery(q) => self.picker = Some((q, None)),
            Msg::PickerSelect(i) => {
                if let Some(p) = &mut self.picker {
                    p.1 = Some(i);
                }
            }
            Msg::PickerAdd => {
                let Some((q, Some(i))) = self.picker.take() else { return };
                cx.close_sheet();
                let Some(choice) = self.registry().and_then(|r| filter(&r.choices, &q).get(i).map(|c| (*c).clone()))
                else {
                    return;
                };
                let name = choice.description.clone();
                cx.change(|s| {
                    let k = &mut s.input.keyboard;
                    let mut list = layouts(k);
                    let item = (choice.layout.clone(), choice.variant.clone().unwrap_or_default());
                    if !list.contains(&item) {
                        list.push(item);
                    }
                    set_layouts(k, &list);
                });
                cx.say(format!("Added {name}"));
            }
            Msg::PickerCancel => {
                self.picker = None;
                cx.close_sheet();
            }
            Msg::RemoveLayout(i) => cx.change(|s| {
                let k = &mut s.input.keyboard;
                let mut list = layouts(k);
                if list.len() > 1 && i < list.len() {
                    list.remove(i);
                }
                set_layouts(k, &list);
            }),
            Msg::MoveUp(i) => cx.change(|s| {
                let k = &mut s.input.keyboard;
                let mut list = layouts(k);
                if i > 0 && i < list.len() {
                    list.swap(i - 1, i);
                }
                set_layouts(k, &list);
            }),
            Msg::Switch(i) => cx.change(|s| {
                let k = &mut s.input.keyboard;
                k.options = replace_options(&k.options, |o| o.starts_with("grp:"), SWITCH.get(i).and_then(|c| c.1));
            }),
            Msg::Delay(v) => {
                cx.change(|s| s.input.keyboard.repeat_delay = ((v / 25.0).round() * 25.0).clamp(150.0, 1000.0) as u32)
            }
            Msg::Rate(v) => cx.change(|s| s.input.keyboard.repeat_rate = v.round().clamp(5.0, 60.0) as u32),
            Msg::Trial(t) => self.trial = t,
            Msg::Modifiers(i) => cx.change(|s| {
                let k = &mut s.input.keyboard;
                k.options = modifiers::set(&k.options, i);
            }),
            Msg::Caps(i) => cx.change(|s| {
                let k = &mut s.input.keyboard;
                k.options = replace_options(&k.options, is_caps_option, CAPS.get(i).and_then(|c| c.1));
            }),
            Msg::Compose(i) => cx.change(|s| {
                let k = &mut s.input.keyboard;
                k.options = replace_options(
                    &k.options,
                    |o| o.starts_with("compose:") && o != "compose:caps",
                    COMPOSE.get(i).and_then(|c| c.1),
                );
            }),
            Msg::NumLock(on) => cx.change(|s| s.input.keyboard.numlock = on),
            Msg::Record(action) => {
                self.recording = Some(action);
                cx.focus("recorder");
            }
            Msg::RecordCancel => self.recording = None,
            Msg::RecordClear => {
                let Some(action) = self.recording.take() else { return };
                cx.change(|s| {
                    let mut map = keys::effective(&s.keys.bindings);
                    map.retain(|_, a| *a != action);
                    s.keys.bindings = keys::overrides_for(&map);
                });
            }
            Msg::Recorded(chord) => {
                let Some(action) = self.recording.take() else { return };
                if !chord.has_modifier() && !chord.stands_alone() {
                    self.recording = Some(action);
                    cx.say("A shortcut needs ❖, ⌃ or ⌥ (or a function key)");
                    return;
                }
                let map = keys::effective(&cx.settings().keys.bindings);
                match map.get(&chord) {
                    Some(other) if *other == action => {}
                    Some(other) => {
                        let other_label = keys::sections(9)
                            .into_iter()
                            .flat_map(|s| s.actions)
                            .find(|(a, _)| a == other)
                            .map_or_else(|| other.clone(), |(_, l)| l);
                        cx.confirm(Confirm::new(
                            format!("{} is “{other_label}”", chord.glyphs().concat()),
                            format!(
                                "Use it for this instead? “{other_label}” will have no shortcut until you give it one."
                            ),
                            "Use It Here",
                            Msg::Rebind(action, chord.to_config()),
                        ));
                    }
                    None => cx.send(Msg::Rebind(action, chord.to_config())),
                }
            }
            Msg::Rebind(action, chord) => {
                let Some(chord) = Chord::parse(&chord) else { return };
                cx.change(|s| {
                    let mut map = keys::effective(&s.keys.bindings);
                    keys::rebind(&mut map, &action, chord);
                    s.keys.bindings = keys::overrides_for(&map);
                });
            }
            Msg::Reset(action) => cx.change(|s| {
                let mut map = keys::effective(&s.keys.bindings);
                map.retain(|_, a| *a != action);
                for (c, a) in keys::effective(&BTreeMap::new()) {
                    if a == action {
                        map.insert(c, a);
                    }
                }
                s.keys.bindings = keys::overrides_for(&map);
            }),
            Msg::RestoreDefaults => cx.confirm(
                Confirm::new(
                    "Restore the default shortcuts?",
                    "Every shortcut you changed goes back to Aizome's, and commands of your own are removed.",
                    "Restore",
                    Msg::RestoreConfirmed,
                )
                .destructive(),
            ),
            Msg::RestoreConfirmed => cx.change(|s| s.keys.bindings.clear()),
            Msg::AddCommand => {
                self.command = Some(CommandSheet::default());
                cx.open_sheet();
                cx.focus("command-text");
            }
            Msg::CommandText(t) => {
                if let Some(c) = &mut self.command {
                    c.command = t;
                    c.error = None;
                }
            }
            Msg::CommandChord(chord) => {
                if let Some(c) = &mut self.command {
                    c.chord = Some(chord);
                    c.error = None;
                }
            }
            Msg::CommandClear => {
                if let Some(c) = &mut self.command {
                    c.chord = None;
                }
            }
            Msg::CommandCancel => {
                self.command = None;
                cx.close_sheet();
            }
            Msg::CommandSave => {
                let Some(c) = &mut self.command else { return };
                let command = c.command.trim().to_owned();
                let Some(chord) = c.chord.clone() else {
                    c.error = Some("Press the keys for it in the field".into());
                    return;
                };
                if !chord.has_modifier() && !chord.stands_alone() {
                    c.error = Some("A shortcut needs ❖, ⌃ or ⌥ (or a function key)".into());
                    return;
                }
                let map = keys::effective(&cx.settings().keys.bindings);
                if let Some(other) = map.get(&chord) {
                    c.error = Some(format!("{} is taken ({other}): choose other keys", chord.glyphs().concat()));
                    return;
                }
                self.command = None;
                cx.close_sheet();
                cx.change(|s| {
                    let mut map = keys::effective(&s.keys.bindings);
                    map.insert(chord, format!("spawn {command}"));
                    s.keys.bindings = keys::overrides_for(&map);
                });
            }
            Msg::RemoveCommand(chord) => {
                let Some(chord) = Chord::parse(&chord) else { return };
                cx.change(|s| {
                    let mut map = keys::effective(&s.keys.bindings);
                    map.remove(&chord);
                    s.keys.bindings = keys::overrides_for(&map);
                });
            }
        }
    }

    fn toolbar(&self, _vcx: &ViewCx) -> Option<Element<Msg>> {
        let i = match self.tab {
            Tab::Typing => 0,
            Tab::Shortcuts => 1,
        };
        Some(form::segments(&["Typing", "Shortcuts"], i, |i| {
            Msg::Tab(if i == 0 { Tab::Typing } else { Tab::Shortcuts })
        }))
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let sections = match self.tab {
            Tab::Typing => self.typing(vcx),
            Tab::Shortcuts => self.shortcuts(vcx),
        };
        form::page(sections)
    }

    fn sheet(&self, _vcx: &ViewCx) -> Option<Element<Msg>> {
        if self.picker.is_some() {
            return Some(self.picker_view());
        }
        self.command.as_ref().map(|c| self.command_view(c))
    }
}
