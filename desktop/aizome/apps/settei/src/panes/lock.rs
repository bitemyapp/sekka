//! Lock Screen: when the screen locks by itself, whether it asks for the
//! password after sleep or the screen going dark, and what it shows, with
//! the lock screen drawn small. `[lock]` and `[idle]`, which the shell and
//! the compositor follow.

use aizome_config::LockSettings;
use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use crate::extra;
use crate::form;
use crate::pane::{Group, Pane, PaneCx, SettingEntry, Tile, ViewCx};
use crate::widgets::{avatar, mark};

#[derive(Default)]
pub struct Lock;

#[derive(Clone, Debug)]
pub enum Msg {
    After(usize),
    ScreenOff(usize),
    BeforeSleep(bool),
    AfterScreenOff(bool),
    Clock(bool),
    Picture(bool),
}

/// Minutes to choose from, 0 for never.
pub const MINUTES: [u32; 9] = [1, 2, 5, 10, 15, 20, 30, 60, 0];

pub fn minutes_label(m: u32) -> String {
    match m {
        0 => "Never".into(),
        1 => "1 minute".into(),
        60 => "1 hour".into(),
        m => format!("{m} minutes"),
    }
}

pub fn minutes_choice(current: u32) -> (Vec<String>, usize) {
    let mut v: Vec<u32> = MINUTES.to_vec();
    if !v.contains(&current) {
        v.insert(v.len() - 1, current);
    }
    let i = v.iter().position(|m| *m == current).unwrap_or(0);
    (v.iter().map(|m| minutes_label(*m)).collect(), i)
}

/// The value chosen at `i` of `minutes_choice(current)`.
pub fn minutes_at(current: u32, i: usize) -> u32 {
    let mut v: Vec<u32> = MINUTES.to_vec();
    if !v.contains(&current) {
        v.insert(v.len() - 1, current);
    }
    v.get(i).copied().unwrap_or(current)
}

/// The lock screen, small: the time, the date, the person and the field.
fn preview(vcx: &ViewCx, show_clock: bool, show_picture: bool) -> Element<Msg> {
    let me = vcx.sys().accounts.me();
    let mut col = column![].spacing(6.0).align(Align::Center);
    if show_clock {
        col = col
            .push(text("21:42").mono().size(30.0).weight(600.0).tabular().color(Color::rgba8(0xee, 0xf1, 0xff, 0xff)))
            .push(text("Saturday 17 October").size(11.0).color(Color::rgba8(0xc0, 0xca, 0xf5, 0xff)))
            .push(space(10.0));
    }
    let face: Element<Msg> = if show_picture {
        avatar(None, &me.initials(), &me.login, 34.0)
    } else {
        container(text(me.initials()).size(14.0).weight(650.0).color(Color::rgba8(0xee, 0xf1, 0xff, 0xff)))
            .center()
            .width(34.0)
            .height(34.0)
            .fill(Tone::Color(Color::rgba8(0x24, 0x28, 0x3b, 0xff)))
            .into()
    };
    col = col
        .push(face)
        .push(text(me.display_name().to_owned()).size(12.0).weight(650.0).color(Color::rgba8(0xee, 0xf1, 0xff, 0xff)))
        .push(
            container(text("Password").size(11.0).color(Color::rgba8(0x93, 0x9a, 0xba, 0xff)))
                .padding([4.0, 8.0])
                .width(150.0)
                .fill(Tone::Color(Color::rgba8(0x24, 0x28, 0x3b, 0xff)))
                .edges(Edge::Color(Color::rgba8(0xe0, 0xaf, 0x68, 0xff)), Sides::LEFT),
        )
        .push(space(6.0))
        .push(mark(3.0));
    container(col)
        .center()
        .width(Fill)
        .height(230.0)
        .fill(Tone::Color(Color::rgba8(0x16, 0x16, 0x1e, 0xff)))
        .border(Edge::BevelLo)
        .into()
}

impl Pane for Lock {
    type Msg = Msg;

    fn id(&self) -> &'static str {
        "lock"
    }

    fn title(&self) -> &'static str {
        "Lock Screen"
    }

    fn icon(&self) -> Icon {
        Icon::Lock
    }

    fn tile(&self) -> Tile {
        Tile::Kon
    }

    fn group(&self) -> Group {
        Group::Security
    }

    fn keywords(&self) -> &'static [&'static str] {
        &["screen lock", "idle", "password", "security", "screensaver"]
    }

    fn entries(&self) -> Vec<SettingEntry> {
        vec![
            SettingEntry::new("lock-after", "Lock the screen after").keywords(&[
                "idle",
                "inactive",
                "timeout",
                "auto lock",
            ]),
            SettingEntry::new("screen-off", "Turn the screen off after").keywords(&["display off", "blank", "dpms"]),
            SettingEntry::new("before-sleep", "Ask for the password after sleep").keywords(&["suspend", "wake"]),
            SettingEntry::new("after-screen-off", "Ask for the password after the screen goes off")
                .keywords(&["blank"]),
            SettingEntry::new("show-clock", "Show the clock").keywords(&["time", "lock screen"]),
            SettingEntry::new("show-picture", "Show your picture").keywords(&["avatar"]),
        ]
    }

    fn update(&mut self, msg: Msg, cx: &mut PaneCx<Msg>) {
        match msg {
            Msg::After(i) => cx.change(|s| s.idle.lock_after_minutes = minutes_at(s.idle.lock_after_minutes, i)),
            Msg::ScreenOff(i) => {
                cx.change(|s| s.idle.monitors_off_after_minutes = minutes_at(s.idle.monitors_off_after_minutes, i))
            }
            Msg::BeforeSleep(on) => cx.change(|s| {
                let mut l: LockSettings = s.lock_settings();
                l.before_sleep = on;
                s.set_shell_section("lock", &l);
            }),
            Msg::AfterScreenOff(on) => cx.change(|s| extra::set(&mut s.lock, "after_monitors_off", on)),
            Msg::Clock(on) => cx.change(|s| extra::set(&mut s.lock, "show_clock", on)),
            Msg::Picture(on) => cx.change(|s| {
                let mut l: LockSettings = s.lock_settings();
                l.show_picture = on;
                s.set_shell_section("lock", &l);
            }),
        }
    }

    fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let s = vcx.settings();
        let l = s.lock_settings();
        let clock = extra::get_bool(&s.lock, "show_clock", true);
        let after_off = extra::get_bool(&s.lock, "after_monitors_off", true);
        let (lock_opts, lock_i) = minutes_choice(s.idle.lock_after_minutes);
        let (off_opts, off_i) = minutes_choice(s.idle.monitors_off_after_minutes);
        let warn = s.idle.lock_after_minutes == 0
            || (s.idle.monitors_off_after_minutes != 0
                && s.idle.lock_after_minutes > s.idle.monitors_off_after_minutes
                && !after_off);
        let mut when = vec![
            form::row("Lock the screen after", dropdown(lock_opts, lock_i).on_select(Msg::After).min_width(150.0))
                .entry("lock-after")
                .help("Without a key pressed or the pointer moved. ⌃❖Q locks at once."),
            form::row(
                "Turn the screen off after",
                dropdown(off_opts, off_i).on_select(Msg::ScreenOff).min_width(150.0),
            )
            .entry("screen-off"),
            form::row("Ask for the password after sleep", toggle(l.before_sleep).on_toggle(Msg::BeforeSleep))
                .entry("before-sleep"),
            form::row("…and after the screen goes off", toggle(after_off).on_toggle(Msg::AfterScreenOff))
                .entry("after-screen-off"),
        ];
        if warn {
            when.push(form::whole(
                container(
                    row![
                        icon(Icon::TriangleAlert).size(14.0).role(Role::Warning),
                        text("Anyone can use this computer while you're away from it.").size(12.0).role(Role::Warning)
                    ]
                    .spacing(8.0)
                    .align(Align::Center),
                )
                .padding([8.0, form::INSET]),
            ));
        }
        form::page(vec![
            vcx.section(
                "",
                vec![form::whole(
                    container(preview(vcx, clock, l.show_picture)).padding([12.0, form::INSET]).width(Fill),
                )],
            ),
            vcx.section("When", when),
            vcx.section(
                "Showing",
                vec![
                    form::row("Show the clock", toggle(clock).on_toggle(Msg::Clock)).entry("show-clock"),
                    form::row("Show your picture", toggle(l.show_picture).on_toggle(Msg::Picture))
                        .entry("show-picture")
                        .help("Off, your initials stand in."),
                ],
            ),
        ])
    }
}
