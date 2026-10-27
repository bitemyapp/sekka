//! The bar's menus as a menu bar: the open title stays highlighted until
//! its menu closes, the pointer slides from title to title while one is
//! open, ← and → go across the titles, F10 (or ❖⌥M) opens the first from
//! the keyboard, and typing in the Toolchest turns it into the palette.

use std::time::{Duration, Instant};

use aizome_kit::MenuStyle;
use aizome_kit::layout::Rect;
use aizome_kit::prelude::*;

use crate::menubar::Slot;
use crate::shell::{Msg, Shell};

/// A menu of a bar that's open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BarMenu {
    pub bar: WindowKey,
    pub slot: Slot,
    /// Which showing it is: a closing heard late is an older one's.
    pub showing: u64,
}

impl Shell {
    /// `style` as a bar menu's: it says when it closes, and ← and → go to
    /// the titles beside it; from the keyboard, its first row is lit.
    pub(crate) fn as_bar_menu(
        &mut self,
        style: MenuStyle<Msg>,
        bar: WindowKey,
        slot: Slot,
        keyboard: bool,
    ) -> MenuStyle<Msg> {
        self.menu_gen += 1;
        self.bar_menu = Some(BarMenu { bar, slot, showing: self.menu_gen });
        let style = style.on_closed(Msg::MenuClosed(self.menu_gen)).on_side(Msg::BarSide);
        if keyboard { style.from_keyboard() } else { style }
    }

    /// A menu closed: if it's the bar's open one, its title goes back.
    pub(crate) fn menu_closed(&mut self, showing: u64) {
        if let Some(m) = self.bar_menu
            && m.showing == showing
        {
            self.bar_menu = None;
            self.just_closed = Some((m.bar, m.slot, Instant::now()));
        }
    }

    /// A press on a title whose menu that very press closed (a press in
    /// the bar closes its menus first, so the closing comes right before
    /// the press, in the same moment): the menu stays closed.
    pub(crate) fn pressed_to_close(&mut self, bar: WindowKey, slot: Slot) -> bool {
        matches!(self.just_closed.take(), Some((b, s, at)) if b == bar && s == slot && at.elapsed() < Duration::from_millis(50))
    }

    /// The pointer came onto another title while a menu is open: that
    /// title's menu instead.
    pub(crate) fn hover_title(&mut self, bar: WindowKey, slot: Slot, cx: &mut Cx<Msg>) {
        let Some(m) = self.bar_menu else { return };
        if m.bar != bar || m.slot == slot || !matches!(slot, Slot::Toolchest | Slot::Menu(_)) {
            return;
        }
        if let Some((_, rect)) = self.titles(bar, cx).into_iter().find(|(s, _)| *s == slot) {
            self.open_slot(bar, slot, rect, false, cx);
        }
    }

    /// ← or → past the open menu: the title beside it, round the ends.
    pub(crate) fn bar_side(&mut self, d: i32, cx: &mut Cx<Msg>) {
        let Some(m) = self.bar_menu else { return };
        let titles = self.titles(m.bar, cx);
        let Some(i) = titles.iter().position(|(s, _)| *s == m.slot) else { return };
        let (slot, rect) = titles[(i as i32 + d).rem_euclid(titles.len() as i32) as usize];
        self.open_slot(m.bar, slot, rect, true, cx);
    }

    /// F10 or ❖⌥M: the menu bar of `output` (else the focused output's)
    /// takes the keyboard, its app menu open with the first row lit. A
    /// notification's line gives way to the menus.
    pub fn focus_menu_bar(&mut self, output: Option<&str>, cx: &mut Cx<Msg>) -> bool {
        let screen = output.and_then(|n| self.screen_named(n)).or(self.focused_screen()).or(self.primary());
        let Some(bar) = screen.and_then(|s| s.bar) else { return false };
        if let Some(id) = self.banner {
            self.banner_done(id, cx);
        }
        let titles = self.titles(bar, cx);
        let Some(&(slot, rect)) = titles.iter().find(|(s, _)| *s == Slot::Menu(0)).or(titles.first()) else {
            return false;
        };
        self.open_slot(bar, slot, rect, true, cx);
        true
    }

    /// Where `bar`'s titles are now.
    pub(crate) fn titles(&mut self, bar: WindowKey, cx: &Cx<Msg>) -> Vec<(Slot, Rect)> {
        let theme = cx.theme().clone();
        self.title_rects(bar, &theme)
    }

    /// Where `bar`'s titles are now, in `theme`.
    pub fn title_rects(&mut self, bar: WindowKey, theme: &aizome_kit::Theme) -> Vec<(Slot, Rect)> {
        let Some(screen) = self.screens.iter().find(|s| s.bar == Some(bar)).cloned() else { return Vec::new() };
        let model = self.bar_model(&screen);
        let text = &mut self.text;
        let mut measure = |s: &str, spec: &aizome_kit::text::TextSpec| text.layout(theme, s, spec, None).size().width;
        crate::menubar::title_rects(theme, &mut measure, &model)
    }

    /// Typing in the Toolchest: the palette, with what was typed. The
    /// Toolchest stays open, holding the keys, until the palette shows
    /// (the compositor closes it then), so keys typed meanwhile follow the
    /// first into the palette instead of reaching the window underneath.
    pub(crate) fn toolchest_typed(&mut self, text: String, cx: &mut Cx<Msg>) {
        match &mut self.palette {
            Some(p) => {
                let query = format!("{}{text}", p.query);
                self.open_palette(None, query, cx);
            }
            None => {
                self.open_palette(None, text, cx);
                cx.after(std::time::Duration::from_millis(250), Msg::ToolchestHandedOver);
            }
        }
    }
}
