//! Software Update: what this system is, and Yukimi's Updates to change it.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use aizome_kit::prelude::*;
use aizome_kit::{column, row};

use super::Msg as G;
use crate::form;
use crate::pane::{PaneCx, SettingEntry, ViewCx};
use crate::widgets::tile;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct System {
    /// "Sekka 26.11 (Zokor)".
    pub name: String,
    /// "26.11.20261011.7fd4004".
    pub build: String,
    /// The system profile's generation.
    pub generation: Option<u32>,
    /// When it was made current.
    pub since: Option<SystemTime>,
}

#[derive(Default)]
pub struct State {
    system: Option<System>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(System),
    Open(&'static str),
}

pub fn entries() -> Vec<SettingEntry> {
    vec![SettingEntry::new("updates", "Check for updates").under("Software Update").keywords(&[
        "upgrade",
        "yukimi",
        "new version",
    ])]
}

/// This system, read from `/etc/os-release` and the system profile.
pub fn read() -> System {
    let os = aizome_sys::sysinfo::os_release();
    let link = std::fs::read_link("/nix/var/nix/profiles/system").ok();
    let generation = link
        .as_ref()
        .and_then(|l| l.file_name()?.to_str()?.strip_prefix("system-")?.strip_suffix("-link")?.parse().ok());
    let since = std::fs::symlink_metadata("/nix/var/nix/profiles/system").and_then(|m| m.modified()).ok();
    System { name: os.pretty_name.clone(), build: os.build_id.clone().unwrap_or_default(), generation, since }
}

/// "3 days ago", "today".
pub fn ago(t: SystemTime, now: SystemTime) -> String {
    let d = now.duration_since(t).unwrap_or(Duration::ZERO).as_secs();
    match d / 86400 {
        0 => "today".into(),
        1 => "yesterday".into(),
        n if n < 60 => format!("{n} days ago"),
        n => format!("{} months ago", n / 30),
    }
}

impl State {
    pub fn shown(&mut self, cx: &mut PaneCx<G>) {
        if cx.sample() {
            self.system = Some(System {
                name: "Sekka 26.11 (Zokor)".into(),
                build: "26.11.20261011.7fd4004".into(),
                generation: Some(214),
                since: Some(UNIX_EPOCH + Duration::from_secs(1_791_676_800)),
            });
        } else {
            cx.spawn(|| G::Update(Msg::Read(read())));
        }
    }

    pub fn update(&mut self, msg: Msg, cx: &mut PaneCx<G>) {
        match msg {
            Msg::Read(s) => self.system = Some(s),
            Msg::Open(page) => cx.launch(&["aizome-yukimi", "--page", page]),
        }
    }

    pub fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let s = self.system.clone().unwrap_or_default();
        let now = if vcx.sample() { UNIX_EPOCH + Duration::from_secs(1_792_195_200) } else { SystemTime::now() };
        let mut facts = column![
            text(if s.name.is_empty() { "This system".to_owned() } else { s.name.clone() }).size(15.0).weight(650.0)
        ]
        .spacing(4.0);
        if !s.build.is_empty() {
            facts = facts.push(text(s.build.clone()).mono().size(11.0).role(Role::Text3));
        }
        let mut line = Vec::new();
        if let Some(g) = s.generation {
            line.push(format!("Generation {g}"));
        }
        if let Some(t) = s.since {
            line.push(format!("installed {}", ago(t, now)));
        }
        if !line.is_empty() {
            facts = facts.push(text(line.join(", ")).size(12.0).role(Role::Text2));
        }
        let hero = row![tile(Icon::Snowflake, crate::pane::Tile::Hanada, 44.0), facts.width(Fill)]
            .spacing(14.0)
            .align(Align::Center);
        form::page(vec![
            vcx.section(
                "",
                vec![
                    form::whole(container(hero).padding([14.0, form::INSET]).width(Fill)),
                    form::row("Updates", button("Check in Yukimi").primary().icon(Icon::RefreshCw).on_press(Msg::Open("updates")))
                        .entry("updates")
                        .help("Yukimi, Aizome's software manager, checks for new versions of the system and your apps, shows what changes, and installs them."),
                    form::row("Earlier versions", button("History").on_press(Msg::Open("history")))
                        .help("Every update keeps the version before it: start an older one from the boot menu, or go back in Yukimi."),
                ],
            ),
        ])
    }
}
