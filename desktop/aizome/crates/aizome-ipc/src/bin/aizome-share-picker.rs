//! `aizome-share-picker`: what xdg-desktop-portal-wlr asks, in Aizome
//! sessions, which screen or window to share (its `chooser_cmd`, with
//! `chooser_type=dmenu`).
//!
//! The portal lists what may be shared on stdin, a line each
//! (`Monitor: NAME DESCRIPTION`, `Window: TITLE (IDENTIFIER)`). This opens
//! the compositor's picker with a banner saying what it's for (IPC
//! `pick-area`, displays and windows only) and prints the line for what
//! the person picks: the display clicked, or the window clicked after
//! Space. Esc prints nothing, so the portal refuses the share.
//!
//! Given no list (`chooser_type=simple`) any display or window may be
//! picked, and it prints `Monitor: NAME` or `Window: IDENTIFIER`.

use std::io::Read;
use std::process::ExitCode;

use aizome_ipc::{Client, Event, PickTarget, Request};

/// What the portal offers: displays by name, windows by their
/// ext-foreign-toplevel-list identifier, each with its line.
#[derive(Debug, Default, PartialEq)]
struct Offer {
    monitors: Vec<(String, String)>,
    windows: Vec<(String, String)>,
}

impl Offer {
    fn parse(list: &str) -> Offer {
        let mut offer = Offer::default();
        for line in list.lines().map(str::trim_end).filter(|l| !l.is_empty()) {
            if let Some(rest) = line.strip_prefix("Monitor: ") {
                let name = rest.split(' ').next().unwrap_or(rest);
                offer.monitors.push((name.to_owned(), line.to_owned()));
            } else if let Some(rest) = line.strip_prefix("Window: ") {
                // `TITLE (IDENTIFIER)`: the identifier is in the last
                // parentheses (a title may have its own).
                let id = rest.strip_suffix(')').and_then(|r| r.rfind(" (").map(|i| &r[i + 2..])).unwrap_or(rest);
                offer.windows.push((id.to_owned(), line.to_owned()));
            }
        }
        offer
    }

    /// No list: the portal's `simple` chooser.
    fn simple(&self) -> bool {
        self.monitors.is_empty() && self.windows.is_empty()
    }

    /// What may be picked.
    fn targets(&self) -> Vec<PickTarget> {
        let mut t = Vec::new();
        if self.simple() || !self.monitors.is_empty() {
            t.push(PickTarget::Display);
        }
        if self.simple() || !self.windows.is_empty() {
            t.push(PickTarget::Window);
        }
        t
    }

    /// The banner's words.
    fn prompt(&self) -> &'static str {
        match self.targets()[..] {
            [PickTarget::Display] => "Choose a screen to share — Esc to cancel",
            [PickTarget::Window] => "Choose a window to share — Esc to cancel",
            _ => "Choose a screen or window to share — Esc to cancel",
        }
    }

    /// The line to print for a pick: the window's, else its display's.
    fn answer(&self, output: Option<&str>, toplevel: Option<&str>) -> Option<String> {
        if self.simple() {
            return match (toplevel, output) {
                (Some(t), _) => Some(format!("Window: {t}")),
                (None, Some(o)) => Some(format!("Monitor: {o}")),
                (None, None) => None,
            };
        }
        let window = toplevel.and_then(|t| self.windows.iter().find(|(id, _)| id == t));
        let monitor = output.and_then(|o| self.monitors.iter().find(|(name, _)| name == o));
        window.or(monitor).map(|(_, line)| line.clone())
    }
}

fn main() -> ExitCode {
    let mut list = String::new();
    let _ = std::io::stdin().read_to_string(&mut list);
    let offer = Offer::parse(&list);
    match pick(&offer) {
        Ok(Some(line)) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Ok(None) => ExitCode::from(1),
        Err(e) => {
            eprintln!("aizome-share-picker: {e}");
            ExitCode::from(1)
        }
    }
}

/// Opens the compositor's picker and waits for the pick (`None`: given
/// up, or nothing on offer picked).
fn pick(offer: &Offer) -> std::io::Result<Option<String>> {
    let tag = format!("share-{}", std::process::id());
    // Listening before asking, so the pick can't come first.
    let events = Client::connect()?.events()?;
    Client::connect()?.call(&Request::PickArea {
        tag: tag.clone(),
        prompt: Some(offer.prompt().to_owned()),
        targets: offer.targets(),
    })?;
    for e in events {
        if let Event::AreaPicked { tag: t, area, output, toplevel, .. } = e?
            && t == tag
        {
            if area.is_none() {
                eprintln!("aizome-share-picker: cancelled");
                return Ok(None);
            }
            let line = offer.answer(output.as_deref(), toplevel.as_deref());
            if line.is_none() {
                eprintln!("aizome-share-picker: {output:?} {toplevel:?} isn't on offer");
            }
            return Ok(line);
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "the compositor went away"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: &str = "Monitor: eDP-1 BOE 0x0BCA\nMonitor: DP-2 Dell U2723QE (DP-2)\n\
        Window: Inbox (3) — Mail (k8Q2xY7p)\nWindow: foot (Zz91aa0b)\n";

    #[test]
    fn reads_what_the_portal_offers() {
        let o = Offer::parse(LIST);
        assert_eq!(o.monitors.iter().map(|m| m.0.as_str()).collect::<Vec<_>>(), ["eDP-1", "DP-2"]);
        assert_eq!(o.windows.iter().map(|w| w.0.as_str()).collect::<Vec<_>>(), ["k8Q2xY7p", "Zz91aa0b"]);
        assert_eq!(o.targets(), [PickTarget::Display, PickTarget::Window]);
        assert_eq!(o.prompt(), "Choose a screen or window to share — Esc to cancel");
        let screens = Offer::parse("Monitor: eDP-1 BOE\n");
        assert_eq!(screens.targets(), [PickTarget::Display]);
        assert_eq!(screens.prompt(), "Choose a screen to share — Esc to cancel");
    }

    #[test]
    fn answers_with_the_portals_own_line() {
        let o = Offer::parse(LIST);
        assert_eq!(o.answer(Some("DP-2"), None).as_deref(), Some("Monitor: DP-2 Dell U2723QE (DP-2)"));
        assert_eq!(o.answer(Some("eDP-1"), Some("k8Q2xY7p")).as_deref(), Some("Window: Inbox (3) — Mail (k8Q2xY7p)"));
        // A window not on offer: its display.
        assert_eq!(o.answer(Some("eDP-1"), Some("gone")).as_deref(), Some("Monitor: eDP-1 BOE 0x0BCA"));
        assert_eq!(o.answer(Some("HDMI-A-1"), None), None);
    }

    #[test]
    fn without_a_list_any_display_or_window() {
        let o = Offer::parse("");
        assert!(o.simple());
        assert_eq!(o.targets(), [PickTarget::Display, PickTarget::Window]);
        assert_eq!(o.answer(Some("eDP-1"), None).as_deref(), Some("Monitor: eDP-1"));
        assert_eq!(o.answer(Some("eDP-1"), Some("k8Q2xY7p")).as_deref(), Some("Window: k8Q2xY7p"));
    }
}
