//! Startup: what the computer shows while it starts and stops. Sekka's boot
//! splash is one of the system's own choices (`programs.yukimi.choices`, in
//! `/etc/yukimi/system.json`), which only Yukimi changes: the button
//! proposes it there, and Apply asks for an administrator password and
//! builds the system. The splash shows from the next start.

use aizome_kit::prelude::*;
use aizome_kit::row;

use super::Msg as G;
use crate::form;
use crate::pane::{PaneCx, SettingEntry, ViewCx};
use crate::system::choices;

/// The setting Sekka declares for its boot splash.
pub const SPLASH: &str = "calamares.bootSplash.enable";

/// The boot splash in the running system, and in the one the computer
/// starts next (a change Yukimi prepared waits for a restart).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Splash {
    pub now: bool,
    pub next: bool,
}

#[derive(Default)]
pub struct State {
    /// `None` until read; `Some(None)` when this system doesn't offer one.
    splash: Option<Option<Splash>>,
}

#[derive(Clone, Debug)]
pub enum Msg {
    Read(Option<Splash>),
    /// Propose turning it on (`true`) or off in Yukimi.
    Propose(bool),
}

pub fn entries() -> Vec<SettingEntry> {
    vec![SettingEntry::new("boot-splash", "Boot splash").under("Startup").keywords(&[
        "plymouth",
        "splash",
        "boot",
        "crest",
        "boot messages",
    ])]
}

/// The running system's boot splash (`/etc`) and the next start's (the
/// system profile, which a change prepared for the next start moves).
pub fn read() -> Option<Splash> {
    choices::read(SPLASH).map(|c| Splash { now: c.now == "true", next: c.next == "true" })
}

/// "On", or "Off from the next start" while a change waits for one.
pub fn state(s: Splash) -> &'static str {
    match (s.now == s.next, s.next) {
        (true, true) => "On",
        (true, false) => "Off",
        (false, true) => "On from the next start",
        (false, false) => "Off from the next start",
    }
}

impl State {
    pub fn shown(&mut self, cx: &mut PaneCx<G>) {
        if cx.sample() {
            self.splash = Some(Some(Splash { now: false, next: false }));
        } else {
            cx.spawn(|| G::Startup(Msg::Read(read())));
        }
    }

    pub fn update(&mut self, msg: Msg, cx: &mut PaneCx<G>) {
        match msg {
            Msg::Read(s) => self.splash = Some(s),
            Msg::Propose(on) => cx.launch(&["aizome-yukimi", "--choose", &format!("{SPLASH}={on}")]),
        }
    }

    pub fn view(&self, vcx: &ViewCx) -> Element<Msg> {
        let row = match self.splash {
            None => form::whole(container(progress_indeterminate().width(160.0)).padding([16.0, form::INSET])),
            Some(None) => form::row("Boot splash", text("Not offered").role(Role::Text3))
                .entry("boot-splash")
                .help("Sekka's boot splash comes with Sekka. This system doesn't offer it."),
            Some(Some(s)) => {
                let label = if s.next { "Turn Off…" } else { "Turn On…" };
                let controls = row![text(state(s)).role(Role::Text2), button(label).on_press(Msg::Propose(!s.next))]
                    .spacing(10.0)
                    .align(Align::Center);
                form::row("Boot splash", controls).entry("boot-splash").help(
                    "Sekka's crest while the computer starts and stops; Esc shows the messages. Yukimi makes the \
                     change, which asks for an administrator password, and it shows from the next start.",
                )
            }
        };
        form::page(vec![vcx.section("", vec![row])])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_splash_is_read_from_the_systems_choices() {
        let facts = |value: &str| {
            format!(
                r#"{{"nixpkgs":null,"choices":[
                  {{"setting":"calamares.loginScreen","title":"Login screen","value":"auto","options":[]}},
                  {{"setting":"calamares.bootSplash.enable","title":"Boot splash","boolean":true,"value":"{value}",
                   "options":[{{"id":"true","title":"On"}},{{"id":"false","title":"Off"}}]}}]}}"#
            )
        };
        // Whether a system's `yukimi/system.json` offers it, and if so whether it's on.
        let offered = |text: &str| choices::value(text, SPLASH).map(|v| v == "true");
        assert_eq!(offered(&facts("true")), Some(true));
        assert_eq!(offered(&facts("false")), Some(false));
        // Older Sekka, other systems, or no Yukimi.
        assert_eq!(offered(r#"{"choices":[]}"#), None);
        assert_eq!(offered("{}"), None);
        assert_eq!(offered("not json"), None);
    }

    #[test]
    fn a_change_waiting_for_a_restart_says_so() {
        assert_eq!(state(Splash { now: false, next: false }), "Off");
        assert_eq!(state(Splash { now: true, next: true }), "On");
        assert_eq!(state(Splash { now: false, next: true }), "On from the next start");
        assert_eq!(state(Splash { now: true, next: false }), "Off from the next start");
    }
}
