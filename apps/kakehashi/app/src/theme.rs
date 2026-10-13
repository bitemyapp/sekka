//! Sekka's colours (docs/brand/tokens.json): Night when the system is dark,
//! Day when it is light, following it as it changes.

use gpui_kit::App;
use gpui_kit::component::{Theme, ThemeConfig};
use std::rc::Rc;

/// The component colours, from the brand tokens of one theme.
struct Palette {
    surface: &'static str,
    raised: &'static str,
    selection: &'static str,
    border: &'static str,
    text: &'static str,
    secondary: &'static str,
    strong: &'static str,
    accent: &'static str,
    on_accent: &'static str,
    danger: &'static str,
    success: &'static str,
    warning: &'static str,
}

const NIGHT: Palette = Palette {
    surface: "#1a1b26",
    raised: "#24283b",
    selection: "#292e42",
    border: "#595959aa",
    text: "#a9b1d6",
    secondary: "#939aba",
    strong: "#c0caf5",
    accent: "#7aa2f7",
    on_accent: "#1a1b26",
    danger: "#f7768e",
    success: "#9ece6a",
    warning: "#e0af68",
};

const DAY: Palette = Palette {
    surface: "#e1e2e7",
    raised: "#eef0f5",
    selection: "#cdd1e2",
    border: "#a1a6c5",
    text: "#3156ab",
    secondary: "#47578f",
    strong: "#1a1b26",
    accent: "#1357b5",
    on_accent: "#ffffff",
    danger: "#b3083a",
    success: "#49602f",
    warning: "#705631",
};

fn config(name: &str, mode: &str, p: &Palette) -> ThemeConfig {
    let colors = serde_json::json!({
        "background": p.surface,
        "foreground": p.text,
        "border": p.border,
        "input.border": p.border,
        "ring": p.accent,
        "link": p.accent,
        "selection.background": p.selection,
        "primary.background": p.accent,
        "primary.foreground": p.on_accent,
        "primary.hover.background": p.accent,
        "primary.active.background": p.accent,
        "secondary.background": p.raised,
        "secondary.foreground": p.strong,
        "secondary.hover.background": p.selection,
        "secondary.active.background": p.selection,
        "muted.background": p.raised,
        "muted.foreground": p.secondary,
        "accent.background": p.selection,
        "accent.foreground": p.strong,
        "danger.background": p.danger,
        "danger.foreground": p.on_accent,
        "danger.hover.background": p.danger,
        "danger.active.background": p.danger,
        "success.background": p.success,
        "success.foreground": p.on_accent,
        "warning.background": p.warning,
        "warning.foreground": p.on_accent,
        "progress.bar.background": p.accent,
        "switch.background": p.border,
        "title_bar.background": p.surface,
        "title_bar.border": p.border,
        "window.border": p.border,
        "popover.background": p.raised,
        "popover.foreground": p.strong,
    });
    serde_json::from_value(serde_json::json!({ "name": name, "mode": mode, "colors": colors }))
        .expect("Sekka's theme is a valid theme")
}

/// Use Sekka's Night and Day themes, in the system's current appearance.
pub fn init(cx: &mut App) {
    Theme::update(cx, |theme| {
        theme.dark_theme = Rc::new(config("Sekka Night", "dark", &NIGHT));
        theme.light_theme = Rc::new(config("Sekka Day", "light", &DAY));
    });
    Theme::sync_system_appearance(None, cx);
}
