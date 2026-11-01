//! Choosing a folder from the sofa, laid out as the settings are: places
//! down the left (home, its usual folders, drives plugged in, the whole
//! computer), the place showing lit; on the right where the folder is, in
//! words, then the choice itself with what Chanoma would find there, then
//! the folder's own folders, each with a word on what's in it.
//!
//! The focus: `col` 0 is the places (`row` the place), 1 the list (`row`
//! 0 the choice, then each folder).

use aizome_kit::Icon;
use aizome_kit::layout::{Point, Rect};
use aizome_kit::sidestep_ui::Canvas;
use aizome_kit::theme::{Role, Theme};

use super::look::*;
use super::{Fit, Focus, H, HEAD_Y, MARGIN, TOP, Target, W};

/// What's in the list under the choice.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Body {
    Reading,
    /// The folder couldn't be read: why.
    Failed(String),
    /// Its folders: each name and a word on what's in it.
    Folders(Vec<(String, String)>),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Picker {
    pub title: String,
    pub places: Vec<(String, Icon)>,
    /// The place the folder showing is in.
    pub place: usize,
    /// Where the folder is: "Home", "ROMs", "snes".
    pub crumbs: Vec<String>,
    /// What Chanoma would find here, and whether that's anything (`None`
    /// while it looks).
    pub found: Option<(String, bool)>,
    pub body: Body,
    /// The first row of the list showing.
    pub first: usize,
}

const SIDE_W: f64 = 400.0;
const PLACE_H: f64 = 78.0;
const ROW_H: f64 = 92.0;
const LIST_X: f64 = MARGIN + SIDE_W + 48.0;
const CRUMBS_Y: f64 = TOP + 30.0;
const LIST_TOP: f64 = TOP + 72.0;
const LIST_BOTTOM: f64 = H - 120.0;
const SIDE_TOP: f64 = TOP + 20.0;

/// How many of the list's rows show at once.
pub fn rows_showing() -> usize {
    ((LIST_BOTTOM - LIST_TOP) / ROW_H).floor().max(1.0) as usize
}

/// The first row to show, from `first`, with `row` focused.
pub fn first_for(first: usize, row: usize) -> usize {
    let n = rows_showing();
    if row < first {
        row
    } else if row >= first + n {
        row + 1 - n
    } else {
        first
    }
}

fn places_showing() -> usize {
    ((LIST_BOTTOM - SIDE_TOP) / PLACE_H).floor().max(1.0) as usize
}

impl Picker {
    /// The list's rows: the choice and each folder.
    pub fn rows(&self) -> usize {
        1 + match &self.body {
            Body::Folders(f) => f.len(),
            _ => 0,
        }
    }

    fn row_rect(&self, i: usize) -> Rect {
        let y = LIST_TOP + (i as f64 - self.first as f64) * ROW_H;
        Rect::new(LIST_X, y, W - MARGIN, y + ROW_H - 10.0)
    }

    fn showing(&self, i: usize) -> bool {
        i >= self.first && i < self.first + rows_showing()
    }

    /// The first place showing (the side list scrolls with the focus).
    pub(super) fn side_first(&self, focus: Focus) -> usize {
        let at = if focus.col == 0 { focus.row } else { self.place };
        at.saturating_sub(places_showing() - 1)
    }

    fn place_rect(&self, i: usize, side_first: usize) -> Rect {
        let y = SIDE_TOP + (i as f64 - side_first as f64) * PLACE_H;
        Rect::new(MARGIN, y, MARGIN + SIDE_W, y + PLACE_H - 10.0)
    }

    fn place_showing(&self, i: usize, side_first: usize) -> bool {
        i >= side_first && i < side_first + places_showing()
    }

    pub fn parts(&self, focus: Focus, fit: &Fit) -> Vec<(Rect, u64)> {
        let side = self.side_first(focus);
        let mut out = Vec::new();
        for i in (0..self.places.len()).filter(|i| self.place_showing(*i, side)) {
            let on = focus.col == 0 && focus.row == i;
            out.push((fit.to_window(reach(self.place_rect(i, side))), u64::from(on)));
        }
        for i in (0..self.rows()).filter(|i| self.showing(*i)) {
            let on = focus.col == 1 && focus.row == i;
            out.push((fit.to_window(reach(self.row_rect(i))), u64::from(on)));
        }
        out
    }

    pub fn hit(&self, focus: Focus, p: Point) -> Option<Target> {
        let side = self.side_first(focus);
        if let Some(i) =
            (0..self.places.len()).find(|i| self.place_showing(*i, side) && self.place_rect(*i, side).contains(p))
        {
            return Some(Target::Section(i));
        }
        (0..self.rows()).find(|i| self.showing(*i) && self.row_rect(*i).contains(p)).map(Target::Item)
    }

    pub fn paint(&self, c: &mut Canvas, theme: &Theme, focus: Focus) {
        let head = Rect::new(0.0, 30.0, W, LIST_TOP - 4.0);
        if c.needs(head) {
            text_mid(c, theme, &self.title, MARGIN, HEAD_Y, font(HEADLINE - 4.0, BOLD), theme.text());
            self.paint_crumbs(c, theme);
        }
        let side = self.side_first(focus);
        for (i, (name, glyph)) in self.places.iter().enumerate() {
            if !self.place_showing(i, side) {
                continue;
            }
            let r = self.place_rect(i, side);
            if !c.needs(reach(r)) {
                continue;
            }
            let current = i == self.place;
            if current {
                lit(c, theme, r);
            }
            let ink = if current { theme.on_selection() } else { theme.text() };
            icon(c, *glyph, Point::new(r.x0 + 34.0, r.center().y), 30.0, if current { ink } else { theme.text2() });
            let name = fit_text(theme, name, r.width() - 90.0, font(BODY, MEDIUM));
            text_mid(c, theme, &name, r.x0 + 70.0, r.center().y, font(BODY, MEDIUM), ink);
            if focus.col == 0 && focus.row == i {
                ring(c, theme, r);
            }
        }
        if side > 0 {
            icon(c, Icon::ChevronUp, Point::new(MARGIN + SIDE_W / 2.0, SIDE_TOP - 12.0), 28.0, theme.text3());
        }
        if self.places.len() > side + places_showing() {
            icon(c, Icon::ChevronDown, Point::new(MARGIN + SIDE_W / 2.0, LIST_BOTTOM + 14.0), 28.0, theme.text3());
        }
        for i in (0..self.rows()).filter(|i| self.showing(*i)) {
            let r = self.row_rect(i);
            if !c.needs(reach(r)) {
                continue;
            }
            let on = focus.col == 1 && focus.row == i;
            if i == 0 {
                self.paint_choice(c, theme, r);
            } else if let Body::Folders(f) = &self.body {
                let (name, word) = &f[i - 1];
                face(c, theme, r, theme.raised(), theme.line());
                icon(c, Icon::Folder, Point::new(r.x0 + 28.0 + 18.0, r.center().y), 36.0, theme.text2());
                let room = r.width() - 84.0 - 80.0;
                let spec = font(BODY + 2.0, MEDIUM);
                text_mid(c, theme, &fit_text(theme, name, room, spec), r.x0 + 84.0, r.y0 + 30.0, spec, theme.text());
                let dspec = font(SMALL - 2.0, REGULAR);
                text_mid(c, theme, &fit_text(theme, word, room, dspec), r.x0 + 84.0, r.y1 - 22.0, dspec, theme.text3());
                let arrow = if on { theme.text() } else { theme.text3() };
                icon(c, Icon::ChevronRight, Point::new(r.x1 - 28.0 - 14.0, r.center().y), 32.0, arrow);
            }
            if on {
                ring(c, theme, r);
            }
        }
        // Under the choice, when there are no folders to show.
        let note = Rect::new(LIST_X, LIST_TOP + ROW_H, W - MARGIN, LIST_TOP + 2.0 * ROW_H);
        if self.first == 0 && c.needs(note) {
            let y = note.y0 + 40.0;
            match &self.body {
                Body::Reading => {
                    text_mid(c, theme, "Reading this folder…", LIST_X + 28.0, y, font(BODY, REGULAR), theme.text3());
                }
                Body::Failed(why) => {
                    let danger = theme.role(Role::Danger);
                    icon(c, Icon::TriangleAlert, Point::new(LIST_X + 28.0 + 18.0, y), 36.0, danger);
                    let w = text_mid(
                        c,
                        theme,
                        "Chanoma can't open this folder",
                        LIST_X + 84.0,
                        y,
                        font(BODY, MEDIUM),
                        danger,
                    );
                    let why = fit_text(theme, why, note.width() - 84.0 - w - 24.0, font(SMALL - 2.0, REGULAR));
                    text_mid(
                        c,
                        theme,
                        &why,
                        LIST_X + 84.0 + w + 24.0,
                        y + 2.0,
                        font(SMALL - 2.0, REGULAR),
                        theme.text3(),
                    );
                }
                Body::Folders(f) if f.is_empty() => {
                    text_mid(c, theme, "No folders in here", LIST_X + 28.0, y, font(BODY, REGULAR), theme.text3());
                }
                Body::Folders(_) => {}
            }
        }
        let mid = (LIST_X + W - MARGIN) / 2.0;
        if self.first > 0 {
            icon(c, Icon::ChevronUp, Point::new(mid, LIST_TOP - 12.0), 28.0, theme.text3());
        }
        if self.rows() > self.first + rows_showing() {
            icon(c, Icon::ChevronDown, Point::new(mid, LIST_BOTTOM + 14.0), 28.0, theme.text3());
        }
    }

    /// Where the folder is: "Home › ROMs › snes", the folder itself
    /// strongest, the start cut away if it's long.
    fn paint_crumbs(&self, c: &mut Canvas, theme: &Theme) {
        let spec = font(SMALL + 2.0, REGULAR);
        let last_spec = font(SMALL + 2.0, SEMIBOLD);
        let sep = "  ›  ";
        let room = W - MARGIN - LIST_X;
        let width = |crumbs: &[String]| -> f64 {
            crumbs
                .iter()
                .enumerate()
                .map(|(i, s)| measure(theme, s, if i + 1 == crumbs.len() { last_spec } else { spec }).width)
                .sum::<f64>()
                + measure(theme, sep, spec).width * crumbs.len().saturating_sub(1) as f64
        };
        let mut crumbs = self.crumbs.clone();
        while crumbs.len() > 2 && width(&crumbs) > room {
            crumbs.remove(0);
            crumbs[0] = "…".into();
        }
        let mut x = LIST_X;
        for (i, s) in crumbs.iter().enumerate() {
            let last = i + 1 == crumbs.len();
            if i > 0 {
                x += text_mid(c, theme, sep, x, CRUMBS_Y, spec, theme.text3());
            }
            let (sp, ink) = if last { (last_spec, theme.text()) } else { (spec, theme.text2()) };
            let s = if last { fit_text(theme, s, LIST_X + room - x, sp) } else { s.clone() };
            x += text_mid(c, theme, &s, x, CRUMBS_Y, sp, ink);
        }
    }

    /// The choice: use the folder showing, with what's in it.
    fn paint_choice(&self, c: &mut Canvas, theme: &Theme, r: Rect) {
        face(c, theme, r, theme.raised(), theme.line2());
        let (word, any) = match &self.found {
            Some((w, any)) => (w.as_str(), *any),
            None => ("Looking…", false),
        };
        let mark = if any { theme.role(Role::Success) } else { theme.text3() };
        icon(c, Icon::CircleCheck, Point::new(r.x0 + 28.0 + 18.0, r.center().y), 36.0, mark);
        let room = r.width() - 84.0 - 28.0;
        let spec = font(BODY + 2.0, SEMIBOLD);
        text_mid(c, theme, "Use this folder", r.x0 + 84.0, r.y0 + 30.0, spec, theme.text());
        let dspec = font(SMALL - 2.0, REGULAR);
        let ink = if any { theme.text2() } else { theme.text3() };
        text_mid(c, theme, &fit_text(theme, word, room, dspec), r.x0 + 84.0, r.y1 - 22.0, dspec, ink);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_list_scrolls_to_keep_the_focus_in_sight() {
        let n = rows_showing();
        assert!(n >= 6, "{n} rows show");
        assert_eq!(first_for(0, 0), 0);
        assert_eq!(first_for(0, n - 1), 0);
        assert_eq!(first_for(0, n), 1);
        assert_eq!(first_for(5, 2), 2);
    }
}
