//! Keyboard: layouts (searched from xkeyboard-config's registry, with
//! variants) and the keys that switch them; key repeat with a place to try
//! it; modifier swaps, Caps Lock as Control, Escape or Compose and the
//! Compose key, every keyboard's or one keyboard's own, with the setup
//! that finds out where its ❖ should go (`setup.rs`); and the editor for
//! the system's shortcuts (the compositor's `[keys]`). `[input.keyboard]`,
//! `[[input.keyboards]]` and `[keys.bindings]`, applied by the compositor.

pub mod keys;
mod modifiers;
mod record;
mod setup;

use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use aizome_config::{KeyboardProfile, Settings, key_options};
use aizome_ipc::KeyboardInfo;
use aizome_kit::prelude::*;
use aizome_kit::{column, row};
use aizome_sys::keyboard::KeyboardData;

use crate::form;
use crate::pane::{Confirm, Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use keys::Chord;
use setup::Setup;

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
    /// Whose keys the Keys section shows: a keyboard with its own (name
    /// and ID), or other keyboards'.
    keys_of: Option<(String, String)>,
    /// The setup sheet, while it's open.
    setup: Option<Setup>,
    /// Waits for a key so far: one that isn't the latest is given up.
    waits: Arc<AtomicU64>,
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
    /// Whose keys the Keys section shows: other keyboards', then each
    /// keyboard with its own.
    KeysOf(usize),
    /// The keyboard shown has keys of its own no longer.
    Forget,
    /// Open the setup sheet, with the keyboard a launch named
    /// (`--keyboard`) chosen while no key has come.
    SetupOpen(Option<String>),
    /// Wait `n`'s key and the keyboard it came from, none if given up.
    SetupCaught(u64, Result<Option<(u32, Option<KeyboardInfo>)>, String>),
    /// A key on the sample's keyboard (the sample has no compositor).
    SampleKey(u32),
    SetupAgain,
    SetupModifiers(usize),
    SetupFrom(usize),
    /// The keyboards attached, as the compositor lists them.
    SetupKeyboards(Vec<KeyboardInfo>),
    /// One of those not set up, chosen while no key has come.
    SetupChoose(usize),
    SetupSave,
    /// Set up to type as other keyboards do: the keyboard whose key came,
    /// or else the one chosen.
    SetupSame,
    SetupCancel,
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

/// The options of `who`'s keys: a keyboard's own (with every keyboard's
/// layout switching), or, for any other, `[input.keyboard]`'s.
pub fn options_of(s: &Settings, who: Option<&(String, String)>) -> String {
    match who {
        Some((name, id)) => s.input.options_for(name, id),
        None => s.input.keyboard.options.clone(),
    }
}

/// Changes `who`'s keys: `f` gets their options as [`options_of`] gives
/// them. A keyboard that typed as the others did gets keys of its own.
pub fn change_options(s: &mut Settings, who: Option<&(String, String)>, f: impl FnOnce(&str) -> String) {
    let now = options_of(s, who);
    match who.and_then(|(name, id)| s.input.keyboards.iter_mut().find(|k| k.matches(name, id))) {
        Some(k) => k.options = Some(key_options(&f(&now))),
        None => s.input.keyboard.options = f(&now),
    }
}

/// Gives a keyboard its own keys' options, or (`None`) has it type as
/// other keyboards do.
fn put_keyboard(s: &mut Settings, k: &KeyboardInfo, options: Option<String>) {
    match s.input.keyboards.iter_mut().find(|p| p.matches(&k.name, &k.id)) {
        Some(p) => p.options = options,
        None => s.input.keyboards.push(KeyboardProfile {
            name: k.name.clone(),
            id: k.id.clone(),
            options,
            ..Default::default()
        }),
    }
}

/// The Keys section's menu of keyboards: other keyboards, then each with
/// its own (its IDs too where two share a name).
fn keyboard_labels(s: &Settings) -> Vec<String> {
    let ks = &s.input.keyboards;
    let mut v = vec![if ks.is_empty() { "All keyboards" } else { "Other keyboards" }.to_owned()];
    v.extend(ks.iter().map(|k| {
        if ks.iter().filter(|o| o.name == k.name).count() > 1 {
            format!("{} ({})", k.name, k.id)
        } else {
            k.name.clone()
        }
    }));
    v
}

impl Keyboard {
    fn registry(&self) -> Option<&Registry> {
        self.registry.as_deref()
    }

    /// The keyboard whose keys the Keys section shows, while it has an
    /// entry of its own.
    fn who(&self, s: &Settings) -> Option<&(String, String)> {
        self.keys_of.as_ref().filter(|(name, id)| s.input.keyboard_profile(name, id).is_some())
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

        // The keys: every keyboard's, or the one chosen's own.
        let s = vcx.settings();
        let who = self.who(s);
        let own = options_of(s, who);
        let own_opts = options(&own);
        let has_own = |o: &str| own_opts.iter().any(|x| x == o);
        let caps_i = CAPS.iter().position(|(_, o)| o.is_some_and(has_own)).unwrap_or(0);
        let compose_i = COMPOSE.iter().position(|(_, o)| o.is_some_and(has_own)).unwrap_or(0);
        let modifiers_i = modifiers::selected(&own);
        let mut modifier_labels: Vec<String> = modifiers::CHOICES.iter().map(|(label, _)| (*label).into()).collect();
        if modifiers_i == modifiers::CHOICES.len() {
            modifier_labels.push("Custom".into());
        }
        let profiles = &s.input.keyboards;
        let shown = who.and_then(|(n, i)| profiles.iter().position(|k| k.matches(n, i))).map_or(0, |p| p + 1);
        let mut whose = row![dropdown(keyboard_labels(s), shown).on_select(Msg::KeysOf).min_width(240.0)]
            .spacing(6.0)
            .align(Align::Center);
        if shown > 0 {
            whose = whose.push(
                icon_button(Icon::Minus).toolbar().on_press(Msg::Forget).tooltip("Forget its own keys").id("forget"),
            );
        }
        // `keyboard.setup` is this button, which opens the setup sheet.
        whose = whose.push(button("Set Up…").on_press(Msg::SetupOpen(None)).id(vcx.entry_name("setup")));
        let whose_help = match shown {
            0 if profiles.is_empty() => {
                "Set up a keyboard to give it keys of its own: press the key beside its space bar, \
                 and Settei suggests where ❖ goes."
            }
            0 => "For every keyboard without keys of its own.",
            p if profiles[p - 1].options.is_none() => "It types as other keyboards do, until you change one of these.",
            _ => "Its own modifier keys, Caps Lock and Compose key. Layouts, repeat and Num Lock are every keyboard's.",
        };
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
                    form::row("Keyboard", whose).entry("keyboards").help(whose_help),
                    form::row(
                        "Modifier keys",
                        dropdown(modifier_labels, modifiers_i).on_select(Msg::Modifiers).min_width(240.0),
                    )
                    .entry("modifiers")
                    .help(
                        "Super (❖) is the Windows or Command key. Swap Alt and Super for a Mac-style position. \
                         Applies immediately to the keyboard above; choose Default to undo. \
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
                    form::row("Num Lock on at start", toggle(k.numlock).on_toggle(Msg::NumLock))
                        .entry("numlock")
                        .help("On every keyboard."),
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

    fn setup_view(&self, s: &Settings, st: &Setup) -> Element<Msg> {
        let mut col = column![
            text("Set Up a Keyboard").size(15.0).weight(650.0),
            form::note("Press the key just left of the space bar, on the keyboard you're setting up."),
        ]
        .spacing(10.0)
        .width(460.0);
        if st.waiting {
            col = col.push(
                row![icon(Icon::Keyboard).size(16.0).role(Role::Text3), text("Waiting for the key…").role(Role::Text3)]
                    .spacing(8.0)
                    .align(Align::Center),
            );
        }
        // One with no space bar to press beside (a security key, a mouse's
        // receiver) can be chosen instead, to type as the others do.
        if st.keyboard.is_none() && !st.unset.is_empty() {
            let labels: Vec<String> = st
                .unset
                .iter()
                .map(|k| {
                    if st.unset.iter().filter(|o| o.name == k.name).count() > 1 {
                        format!("{} ({})", k.name, k.id)
                    } else {
                        k.name.clone()
                    }
                })
                .collect();
            col = col.push(form::note(
                "No space bar on it, as on a security key or a mouse's receiver? Choose it, then \
                 Same as Other Keyboards.",
            ));
            col = col.push(
                row![
                    text("Keyboard").width(170.0),
                    dropdown(labels, st.chosen).on_select(Msg::SetupChoose).min_width(240.0).id("setup-keyboard")
                ]
                .spacing(8.0)
                .align(Align::Center),
            );
        }
        if let (Some(k), Some(b)) = (&st.keyboard, &st.beside) {
            col = col.push(
                container(
                    column![
                        row![
                            icon(Icon::Keyboard).size(16.0),
                            text(k.name.clone()).weight(650.0),
                            text(k.id.clone()).mono().size(11.0).role(Role::Text3),
                            spacer(),
                            button("Press Again").compact().on_press(Msg::SetupAgain),
                        ]
                        .spacing(8.0)
                        .align(Align::Center)
                        .width(Fill),
                        text(format!("Beside the space bar: {}", b.what)),
                        text(b.why).size(11.5).role(Role::Text2),
                    ]
                    .spacing(6.0),
                )
                .padding([10.0, 12.0])
                .fill(Tone::Sunk)
                .border(Edge::Line)
                .width(Fill),
            );
            let mut labels: Vec<String> = modifiers::CHOICES.iter().map(|(l, _)| (*l).into()).collect();
            if st.modifiers == modifiers::CHOICES.len() {
                labels.push("Custom".into());
            }
            col = col.push(
                row![
                    text("Modifier keys").width(170.0),
                    dropdown(labels, st.modifiers)
                        .on_select(Msg::SetupModifiers)
                        .min_width(240.0)
                        .id("setup-modifiers")
                ]
                .spacing(8.0)
                .align(Align::Center),
            );
            // Its Caps Lock and Compose key: as other keyboards', or as
            // another keyboard's.
            let sources = setup::sources(s);
            if sources.len() > 1 {
                let labels: Vec<String> = sources.into_iter().map(|s| s.label).collect();
                col = col.push(
                    row![
                        text("Caps Lock and Compose").width(170.0),
                        dropdown(labels, st.from).on_select(Msg::SetupFrom).min_width(240.0).id("setup-from")
                    ]
                    .spacing(8.0)
                    .align(Align::Center),
                );
            }
        }
        let ready = st.keyboard.is_some();
        let known = ready || !st.unset.is_empty();
        col.push(text(st.error.clone().unwrap_or_default()).size(11.0).role(Role::Danger))
            .push(
                row![
                    spacer(),
                    button("Not Now").cancel().on_press(Msg::SetupCancel),
                    button("Same as Other Keyboards").enabled(known).on_press(Msg::SetupSame),
                    button("Save").primary().default().enabled(ready).on_press(Msg::SetupSave),
                ]
                .spacing(8.0)
                .width(Fill),
            )
            .into()
    }

    /// Wait for the key beside the space bar: the compositor catches it
    /// (the sample's comes as [`Msg::SampleKey`]).
    fn wait_for_key(&mut self, cx: &mut PaneCx<Msg>) {
        let n = self.waits.fetch_add(1, Ordering::SeqCst) + 1;
        if let Some(st) = &mut self.setup {
            st.waiting = true;
        }
        if cx.sample() {
            return;
        }
        let waits = self.waits.clone();
        cx.spawn(move || Msg::SetupCaught(n, setup::catch_key(n, &waits)));
    }

    fn stop_waiting(&mut self) {
        self.waits.fetch_add(1, Ordering::SeqCst);
        if let Some(st) = &mut self.setup {
            st.waiting = false;
        }
    }

    /// The keyboard set up gets keys of its own (`own`: the modifier keys
    /// chosen, the rest from where the sheet says), or types as other
    /// keyboards do (the one chosen, if no key came); the Keys section
    /// shows it after.
    fn finish_setup(&mut self, own: bool, cx: &mut PaneCx<Msg>) {
        let Some(st) = &self.setup else { return };
        let chosen = if own { None } else { st.unset.get(st.chosen) };
        let Some(k) = st.keyboard.as_ref().or(chosen).cloned() else { return };
        let options = own.then(|| {
            let sources = setup::sources(cx.settings());
            let from = sources.get(st.from).map(|s| s.options.clone()).unwrap_or_default();
            key_options(&modifiers::set(&from, st.modifiers))
        });
        let said = if own {
            format!("{} has keys of its own", k.name)
        } else {
            format!("{} types as other keyboards do", k.name)
        };
        self.close_setup(cx);
        self.keys_of = Some((k.name.clone(), k.id.clone()));
        cx.change(|s| put_keyboard(s, &k, options));
        cx.say(said);
    }

    fn close_setup(&mut self, cx: &mut PaneCx<Msg>) {
        self.stop_waiting();
        self.setup = None;
        cx.close_sheet();
    }

    /// The key beside the space bar came, from `keyboard`: what it is says
    /// where ❖ goes. Another key, or one Settei can't place, waits again.
    fn caught(&mut self, code: u32, keyboard: Option<KeyboardInfo>, cx: &mut PaneCx<Msg>) {
        const ESCAPE: u32 = 1;
        if code == ESCAPE {
            self.close_setup(cx);
            return;
        }
        let Some(st) = &mut self.setup else { return };
        st.waiting = false;
        match (setup::beside_space(code), keyboard) {
            (Err(again), _) => st.error = Some(again.into()),
            (Ok(_), None) => {
                st.error = Some("Settei couldn't tell which keyboard that was: press the key again.".into())
            }
            (Ok(b), Some(k)) => {
                let sources = setup::sources(cx.settings());
                // A keyboard set up before keeps its own; others start as
                // other keyboards.
                st.from = sources
                    .iter()
                    .position(|s| s.who.as_ref().is_some_and(|(n, i)| k.name == *n && k.id.eq_ignore_ascii_case(i)))
                    .unwrap_or(0);
                st.modifiers = setup::suggested(&b).unwrap_or_else(|| modifiers::selected(&sources[st.from].options));
                st.keyboard = Some(k);
                st.beside = Some(b);
                st.error = None;
                return;
            }
        }
        self.wait_for_key(cx);
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
            SettingEntry::new("keyboards", "Keyboards with keys of their own").keywords(&[
                "each keyboard",
                "per keyboard",
                "external",
                "bluetooth",
                "usb",
                "apple",
            ]),
            SettingEntry::new("setup", "Set up a keyboard").keywords(&[
                "new keyboard",
                "identify",
                "plugged in",
                "space bar",
                "where super goes",
            ]),
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
        self.stop_waiting();
    }

    fn focus_entry(&mut self, entry: &str, cx: &mut PaneCx<Msg>) {
        self.tab = if entry.starts_with("shortcut") || entry == "commands" { Tab::Shortcuts } else { Tab::Typing };
        // `keyboard.setup`: what the shell opens for a keyboard plugged in,
        // with `--keyboard` naming it.
        if entry != "setup" {
            return;
        }
        let asked = cx.keyboard_named().map(str::to_owned);
        match &mut self.setup {
            None => cx.send(Msg::SetupOpen(asked)),
            // Open already, for another keyboard: this one is chosen, once
            // it's listed.
            Some(st) if asked.is_some() => {
                st.asked = asked;
                if cx.sample() {
                    let unset = std::mem::take(&mut st.unset);
                    st.set_unset(unset);
                } else {
                    cx.spawn(|| Msg::SetupKeyboards(setup::attached()));
                }
            }
            Some(_) => {}
        }
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
            Msg::Modifiers(i) => {
                let who = self.who(cx.settings()).cloned();
                cx.change(|s| change_options(s, who.as_ref(), |o| modifiers::set(o, i)));
            }
            Msg::Caps(i) => {
                let who = self.who(cx.settings()).cloned();
                cx.change(|s| {
                    change_options(s, who.as_ref(), |o| {
                        replace_options(o, is_caps_option, CAPS.get(i).and_then(|c| c.1))
                    })
                });
            }
            Msg::Compose(i) => {
                let who = self.who(cx.settings()).cloned();
                cx.change(|s| {
                    change_options(s, who.as_ref(), |o| {
                        replace_options(
                            o,
                            |o| o.starts_with("compose:") && o != "compose:caps",
                            COMPOSE.get(i).and_then(|c| c.1),
                        )
                    })
                });
            }
            Msg::KeysOf(i) => {
                self.keys_of = i
                    .checked_sub(1)
                    .and_then(|p| cx.settings().input.keyboards.get(p))
                    .map(|k| (k.name.clone(), k.id.clone()));
            }
            Msg::Forget => {
                let Some((name, id)) = self.who(cx.settings()).cloned() else { return };
                self.keys_of = None;
                cx.change(|s| s.input.keyboards.retain(|k| !k.matches(&name, &id)));
                cx.say(format!("{name} types as other keyboards do"));
            }
            Msg::SetupOpen(asked) => {
                // The keyboards not set up, for one that has no key to press.
                let mut st = Setup { asked, ..Default::default() };
                if cx.sample() {
                    st.set_unset(setup::not_set_up(cx.settings(), &setup::sample_keyboards()));
                } else {
                    cx.spawn(|| Msg::SetupKeyboards(setup::attached()));
                }
                self.setup = Some(st);
                cx.open_sheet();
                self.wait_for_key(cx);
            }
            Msg::SetupKeyboards(attached) => {
                let unset = setup::not_set_up(cx.settings(), &attached);
                if let Some(st) = &mut self.setup {
                    st.set_unset(unset);
                }
            }
            Msg::SetupChoose(i) => {
                if let Some(st) = &mut self.setup {
                    st.chosen = i;
                }
            }
            Msg::SetupCaught(n, result) => {
                if n != self.waits.load(Ordering::SeqCst) || self.setup.is_none() {
                    return;
                }
                match result {
                    Ok(Some((code, keyboard))) => self.caught(code, keyboard, cx),
                    Ok(None) => {}
                    Err(e) => {
                        if let Some(st) = &mut self.setup {
                            st.waiting = false;
                            st.error = Some(format!("Settei couldn't ask Aizome for the key: {e}"));
                        }
                    }
                }
            }
            Msg::SampleKey(code) => {
                if cx.sample() && self.setup.as_ref().is_some_and(|st| st.waiting) {
                    self.caught(code, Some(setup::sample_keyboard()), cx);
                }
            }
            Msg::SetupAgain => {
                if let Some(st) = &mut self.setup {
                    st.keyboard = None;
                    st.beside = None;
                }
                self.wait_for_key(cx);
            }
            Msg::SetupModifiers(i) => {
                if let Some(st) = &mut self.setup {
                    st.modifiers = i;
                }
            }
            Msg::SetupFrom(i) => {
                if let Some(st) = &mut self.setup {
                    st.from = i;
                }
            }
            Msg::SetupSave => self.finish_setup(true, cx),
            Msg::SetupSame => self.finish_setup(false, cx),
            Msg::SetupCancel => self.close_setup(cx),
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

    fn sheet(&self, vcx: &ViewCx) -> Option<Element<Msg>> {
        if self.picker.is_some() {
            return Some(self.picker_view());
        }
        if let Some(st) = &self.setup {
            return Some(self.setup_view(vcx.settings(), st));
        }
        self.command.as_ref().map(|c| self.command_view(c))
    }
}
