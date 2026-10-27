//! Screen and window sharing, as the compositor reports it (IPC `shares`
//! and `shares-changed`: browsers' sharing through the portal): the
//! Deskbar's indicator, in the accent, and its menu saying what's shared,
//! with Stop Sharing.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use aizome_ipc::{OutputInfo, Request, ShareInfo, WindowId, WindowInfo};
use aizome_kit::layout::Rect;
use aizome_kit::{Cx, Icon, MenuStyle};

use crate::actions::Action;
use crate::menus::model::{Entry, Menu as ShellMenu};
use crate::shell::{Msg, Shell};

/// A share and since when the shell has known of it.
#[derive(Clone, Debug, PartialEq)]
pub struct Share {
    pub info: ShareInfo,
    pub since: Instant,
}

/// How long a share waits before it shows: no longer, since the
/// compositor reports a share only once it has lasted a second (a
/// screenshot tool's capture of one frame isn't sharing).
pub const SETTLE: Duration = Duration::ZERO;

/// The shares now, keeping when each known one began.
pub fn update(old: &[Share], new: Vec<ShareInfo>, now: Instant) -> Vec<Share> {
    new.into_iter()
        .map(|info| {
            let since = old.iter().find(|s| s.info.id == info.id).map_or(now, |s| s.since);
            Share { info, since }
        })
        .collect()
}

/// The shares that have lasted.
pub fn settled(shares: &[Share], now: Instant) -> Vec<&Share> {
    shares.iter().filter(|s| now.saturating_duration_since(s.since) >= SETTLE).collect()
}

/// A display as people know it: the built-in one, or its model.
pub fn display_label(name: &str, outputs: &[OutputInfo]) -> String {
    let builtin = ["eDP", "LVDS", "DSI"].iter().any(|p| name.starts_with(p));
    if builtin {
        return "Built-in Display".into();
    }
    match outputs.iter().find(|o| o.name == name).map(|o| o.model.trim()).filter(|m| !m.is_empty()) {
        Some(model) => format!("{model} ({name})"),
        None => name.to_owned(),
    }
}

/// What a share shows, for the menu: its glyph and words.
pub fn describe(info: &ShareInfo, outputs: &[OutputInfo], windows: &BTreeMap<WindowId, WindowInfo>) -> (Icon, String) {
    match (&info.output, info.window) {
        (_, Some(id)) => {
            let title = windows
                .get(&id)
                .map(|w| if w.title.trim().is_empty() { w.app_id.clone() } else { w.title.clone() })
                .unwrap_or_else(|| "A window".into());
            (Icon::AppWindow, title)
        }
        (Some(name), None) => (Icon::Monitor, display_label(name, outputs)),
        (None, None) => (Icon::Monitor, "The screen".into()),
    }
}

/// The indicator's menu: what's shared, and stopping it (each share on its
/// own when there are several).
pub fn menu(shares: &[&Share], outputs: &[OutputInfo], windows: &BTreeMap<WindowId, WindowInfo>) -> ShellMenu {
    let mut items = Vec::new();
    let stop = |id| Action::Wm(Request::StopShare { id });
    if let [one] = shares {
        let (glyph, what) = describe(&one.info, outputs, windows);
        let detail = if one.info.cursor { "with the pointer" } else { "" };
        items.push(Entry::action(what, Action::None).glyph(glyph).detail(detail).enabled(false));
        items.push(Entry::separator());
        items.push(Entry::action("Stop Sharing", stop(Some(one.info.id))).glyph(Icon::CircleX));
    } else {
        for s in shares {
            let (glyph, what) = describe(&s.info, outputs, windows);
            items.push(Entry::action(format!("Stop Sharing {what}"), stop(Some(s.info.id))).glyph(glyph));
        }
        items.push(Entry::separator());
        items.push(Entry::action("Stop All Sharing", stop(None)).glyph(Icon::CircleX));
    }
    ShellMenu::new("Sharing", items)
}

impl Shell {
    /// What the compositor says is shared now.
    pub fn shares_changed(&mut self, shares: Vec<ShareInfo>) {
        self.shares = update(&self.shares, shares, Instant::now());
    }

    /// The bar's indicator, while something has been shared a while.
    pub fn share_badge(&self) -> Option<crate::menubar::ShareBadge> {
        let shown = settled(&self.shares, Instant::now());
        (!shown.is_empty()).then_some(crate::menubar::ShareBadge { count: shown.len() })
    }

    /// The indicator was pressed: what's shared, and Stop Sharing.
    pub fn open_share_menu(&mut self, win: aizome_kit::WindowKey, rect: Rect, cx: &mut Cx<Msg>) {
        let now = Instant::now();
        let shown = settled(&self.shares, now);
        if shown.is_empty() {
            return;
        }
        let menu = menu(&shown, &self.wm.outputs, &self.wm.windows);
        let anchor = Rect::new(rect.x0, crate::menubar::HEIGHT - 1.0, rect.x1, crate::menubar::HEIGHT);
        let style = MenuStyle::below().titled("Sharing");
        self.show_menu(win, anchor, &menu, style, cx);
    }

    /// What's shared, for `aizome-shell msg state`.
    pub fn sharing_json(&self) -> serde_json::Value {
        let now = Instant::now();
        serde_json::Value::Array(
            settled(&self.shares, now)
                .into_iter()
                .map(|s| {
                    let (_, what) = describe(&s.info, &self.wm.outputs, &self.wm.windows);
                    serde_json::json!({
                        "id": s.info.id,
                        "output": s.info.output,
                        "window": s.info.window,
                        "what": what,
                    })
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: u64, output: Option<&str>, window: Option<WindowId>) -> ShareInfo {
        ShareInfo { id, output: output.map(Into::into), window, cursor: false }
    }

    #[test]
    fn shares_show_as_reported() {
        // The compositor reports a share once it has lasted: shown at once.
        let t0 = Instant::now();
        let s = update(&[], vec![info(1, Some("eDP-1"), None)], t0);
        assert_eq!(settled(&s, t0).len(), 1);
        // Known shares keep their start; new ones start now.
        let later = t0 + Duration::from_secs(5);
        let s = update(&s, vec![info(1, Some("eDP-1"), None), info(2, None, Some(7))], later);
        assert_eq!(s[0].since, t0);
        assert_eq!(s[1].since, later);
        assert_eq!(settled(&s, later).len(), 2);
        assert!(update(&s, Vec::new(), later).is_empty());
    }

    #[test]
    fn the_menu_says_whats_shared_and_stops_it() {
        let outputs = vec![OutputInfo { name: "DP-2".into(), model: "U2723QE".into(), ..Default::default() }];
        let mut windows = BTreeMap::new();
        windows.insert(7, WindowInfo { id: 7, title: "Inbox — Mail".into(), ..Default::default() });
        assert_eq!(display_label("eDP-1", &outputs), "Built-in Display");
        assert_eq!(display_label("DP-2", &outputs), "U2723QE (DP-2)");
        let t0 = Instant::now();
        let one = [Share { info: info(3, None, Some(7)), since: t0 }];
        let m = menu(&one.iter().collect::<Vec<_>>(), &outputs, &windows);
        let labels: Vec<&str> = m.items.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Inbox — Mail", "", "Stop Sharing"]);
        assert!(!m.items[0].enabled);
        assert_eq!(m.items[2].action, Action::Wm(Request::StopShare { id: Some(3) }));
        let two =
            [Share { info: info(3, None, Some(7)), since: t0 }, Share { info: info(4, Some("DP-2"), None), since: t0 }];
        let m = menu(&two.iter().collect::<Vec<_>>(), &outputs, &windows);
        let labels: Vec<&str> = m.items.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, ["Stop Sharing Inbox — Mail", "Stop Sharing U2723QE (DP-2)", "", "Stop All Sharing"]);
        assert_eq!(m.items[3].action, Action::Wm(Request::StopShare { id: None }));
    }
}
