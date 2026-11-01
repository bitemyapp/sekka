//! The grown-ups' settings: sections down the left, the section's rows on
//! the right. Up and down move between rows; left and right change a
//! choice; the confirm button switches a switch or does an action; Back
//! goes to the sections, and from there out.
//!
//! The focus: `row` is the row (in the list) or the section (at the side),
//! `col` is 0 at the side and 1 in the list.

use aizome_kit::Icon;
use aizome_kit::layout::{Point, Rect};
use aizome_kit::sidestep_ui::Canvas;
use aizome_kit::theme::{Role, Theme};

use super::look::*;
use super::{Fit, Focus, H, HEAD_Y, MARGIN, TOP, Target, W};

/// What's at a row's right.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Value {
    None,
    Switch(bool),
    /// A choice changed with left and right.
    Choice(String),
    /// Something the confirm button does.
    Action,
    /// A note (a count, a state).
    Note(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Row {
    pub label: String,
    pub detail: String,
    pub value: Value,
    /// A heading over the rows that follow: not a row to stop on.
    pub heading: bool,
    /// A corner flag ("New").
    pub badge: Option<String>,
}

impl Row {
    pub fn new(label: impl Into<String>, value: Value) -> Row {
        Row { label: label.into(), detail: String::new(), value, heading: false, badge: None }
    }

    pub fn heading(label: impl Into<String>) -> Row {
        Row { heading: true, ..Row::new(label, Value::None) }
    }

    pub fn detail(mut self, d: impl Into<String>) -> Row {
        self.detail = d.into();
        self
    }

    pub fn badge(mut self, b: impl Into<String>) -> Row {
        self.badge = Some(b.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Page {
    pub title: String,
    pub sections: Vec<String>,
    /// Each section's icon, on a square of its colour.
    pub icons: Vec<(Icon, Role)>,
    pub section: usize,
    pub rows: Vec<Row>,
    /// The first row showing.
    pub first: usize,
    /// Under the title.
    pub note: String,
}

const SIDE_W: f64 = 400.0;
const ROW_H: f64 = 96.0;
const HEAD_H: f64 = 74.0;
const LIST_X: f64 = MARGIN + SIDE_W + 48.0;
const LIST_TOP: f64 = TOP + 20.0;
const LIST_BOTTOM: f64 = H - 120.0;

impl Page {
    pub fn in_list(&self, focus: Focus) -> bool {
        focus.col == 1
    }

    fn row_h(&self, i: usize) -> f64 {
        if self.rows[i].heading { HEAD_H } else { ROW_H }
    }

    /// Row `i`'s rectangle (scrolled).
    fn row_rect(&self, i: usize) -> Rect {
        let y: f64 = LIST_TOP + (self.first..i).map(|j| self.row_h(j)).sum::<f64>();
        Rect::new(LIST_X, y, W - MARGIN, y + self.row_h(i) - 10.0)
    }

    fn showing(&self, i: usize) -> bool {
        i >= self.first && self.row_rect(i).y1 <= LIST_BOTTOM
    }

    /// The first row to show with row `i` focused.
    pub fn first_for(&self, i: usize) -> usize {
        let mut first = self.first.min(i);
        // Keep a heading above the focus in sight with it.
        if first == i && i > 0 && self.rows[i - 1].heading {
            first = i - 1;
        }
        loop {
            let bottom: f64 = LIST_TOP + (first..=i).map(|j| self.row_h(j)).sum::<f64>();
            if bottom <= LIST_BOTTOM || first >= i {
                return first;
            }
            first += 1;
        }
    }

    /// The next row to stop on from `i`, `down` or up.
    pub fn next(&self, i: usize, down: bool) -> usize {
        let mut j = i;
        loop {
            j = if down {
                j + 1
            } else {
                match j.checked_sub(1) {
                    Some(j) => j,
                    None => return i,
                }
            };
            match self.rows.get(j) {
                None => return i,
                Some(r) if !r.heading => return j,
                _ => {}
            }
        }
    }

    /// The first row to stop on.
    pub fn first_row(&self) -> usize {
        self.rows.iter().position(|r| !r.heading).unwrap_or(0)
    }

    fn section_rect(&self, i: usize) -> Rect {
        let y = TOP + 20.0 + i as f64 * 78.0;
        Rect::new(MARGIN, y, MARGIN + SIDE_W, y + 68.0)
    }

    pub fn parts(&self, focus: Focus, fit: &Fit) -> Vec<(Rect, u64)> {
        let mut out = Vec::new();
        for i in 0..self.sections.len() {
            let on = focus.col == 0 && focus.row == i;
            out.push((fit.to_window(self.section_rect(i).inflate(16.0, 16.0)), u64::from(on)));
        }
        for i in (0..self.rows.len()).filter(|i| self.showing(*i)) {
            let on = focus.col == 1 && focus.row == i;
            out.push((fit.to_window(self.row_rect(i).inflate(16.0, 16.0)), u64::from(on)));
        }
        out
    }

    pub fn hit(&self, _focus: Focus, p: Point) -> Option<Target> {
        if let Some(i) = (0..self.sections.len()).find(|i| self.section_rect(*i).contains(p)) {
            return Some(Target::Section(i));
        }
        (0..self.rows.len())
            .find(|i| self.showing(*i) && !self.rows[*i].heading && self.row_rect(*i).contains(p))
            .map(Target::Item)
    }

    pub fn paint(&self, c: &mut Canvas, theme: &Theme, focus: Focus) {
        // The title where Home has its name.
        let head = Rect::new(0.0, 30.0, W, TOP - 10.0);
        if c.needs(head) {
            let w = text_mid(c, theme, &self.title, MARGIN, HEAD_Y, font(HEADLINE - 4.0, BOLD), theme.text());
            if !self.note.is_empty() {
                let spec = font(SMALL + 2.0, REGULAR);
                text_mid(c, theme, &self.note, MARGIN + w + 24.0, HEAD_Y + 3.0, spec, theme.text3());
            }
        }
        // The sections, as Settei's side list: each with its icon on a
        // square of its colour, the current one lit.
        for (i, s) in self.sections.iter().enumerate() {
            let r = self.section_rect(i);
            if !c.needs(r.inflate(16.0, 16.0)) {
                continue;
            }
            let current = i == self.section;
            let on = focus.col == 0 && focus.row == i;
            if current {
                lit(c, theme, r);
            }
            if let Some((glyph, role)) = self.icons.get(i) {
                let sq = Rect::new(r.x0 + 14.0, r.center().y - 20.0, r.x0 + 54.0, r.center().y + 20.0);
                c.fill_rect(sq, theme.role(*role));
                let ink = if theme.is_dark() { theme.surface() } else { aizome_kit::Color::WHITE };
                icon(c, *glyph, sq.center(), 26.0, ink);
            }
            let ink = if current { theme.on_selection() } else { theme.text() };
            text_mid(c, theme, s, r.x0 + 72.0, r.center().y, font(BODY, MEDIUM), ink);
            if on {
                ring(c, theme, r);
            }
        }
        for i in (0..self.rows.len()).filter(|i| self.showing(*i)) {
            let r = self.row_rect(i);
            if !c.needs(r.inflate(16.0, 16.0)) {
                continue;
            }
            let row = &self.rows[i];
            if row.heading {
                caption(c, theme, &row.label, r.x0 + 4.0, r.y1 - 22.0, theme.text3());
                continue;
            }
            let on = focus.col == 1 && focus.row == i;
            face(c, theme, r, theme.raised(), theme.line());
            let value_w = value_width(theme, &row.value);
            let room = r.width() - value_w - 90.0;
            let spec = font(BODY + 2.0, MEDIUM);
            let label = fit_text(theme, &row.label, room, spec);
            let label_y = if row.detail.is_empty() { r.center().y } else { r.y0 + 30.0 };
            let lw = text_mid(c, theme, &label, r.x0 + 28.0, label_y, spec, theme.text());
            if !row.detail.is_empty() {
                let dspec = font(SMALL - 2.0, REGULAR);
                let d = fit_text(theme, &row.detail, room, dspec);
                text_mid(c, theme, &d, r.x0 + 28.0, r.y1 - 24.0, dspec, theme.text3());
            }
            if let Some(b) = &row.badge {
                let bw = caption_width(theme, b, 16.0) + 16.0 * 1.2;
                chip(c, theme, b, r.x0 + 28.0 + lw + 16.0 + bw, label_y, 16.0);
            }
            paint_value(c, theme, &row.value, r, on);
            if on {
                ring(c, theme, r);
            }
        }
        // More below or above.
        let mid = (LIST_X + W - MARGIN) / 2.0;
        if self.first > 0 {
            icon(c, Icon::ChevronUp, Point::new(mid, LIST_TOP - 10.0), 28.0, theme.text3());
        }
        if (0..self.rows.len()).any(|i| i >= self.first && !self.showing(i)) {
            icon(c, Icon::ChevronDown, Point::new(mid, LIST_BOTTOM + 14.0), 28.0, theme.text3());
        }
    }
}

fn value_width(theme: &Theme, v: &Value) -> f64 {
    match v {
        Value::Switch(_) => 100.0,
        Value::Choice(s) => measure(theme, s, font(BODY, SEMIBOLD)).width + 100.0,
        Value::Note(s) => measure(theme, s, font(SMALL + 2.0, REGULAR)).width + 30.0,
        Value::Action => 50.0,
        Value::None => 0.0,
    }
}

fn paint_value(c: &mut Canvas, theme: &Theme, v: &Value, r: Rect, on: bool) {
    let right = r.x1 - 28.0;
    let cy = r.center().y;
    match v {
        Value::Switch(state) => {
            // The kit's switch: a square track and knob, lit when on.
            let track = Rect::new(right - 80.0, cy - 20.0, right, cy + 20.0);
            if *state {
                c.fill_rect(track, theme.selection());
                aizome_kit::draw::border(c, track, theme.bevel_lo(), EDGE);
            } else {
                well(c, theme, track);
            }
            let k = 28.0;
            let x = if *state { track.x1 - 6.0 - k } else { track.x0 + 6.0 };
            let knob = Rect::new(x, cy - k / 2.0, x + k, cy + k / 2.0);
            c.fill_rect(knob, if *state { theme.on_selection() } else { theme.text3() });
            aizome_kit::draw::border(c, knob, theme.bevel_lo(), EDGE);
        }
        Value::Choice(s) => {
            let arrows = if on { theme.text() } else { theme.text3() };
            icon(c, Icon::ChevronRight, Point::new(right - 14.0, cy), 28.0, arrows);
            let w = text_right(c, theme, s, right - 40.0, cy, font(BODY, SEMIBOLD), theme.text());
            icon(c, Icon::ChevronLeft, Point::new(right - 40.0 - w - 26.0, cy), 28.0, arrows);
        }
        Value::Note(s) => {
            text_right(c, theme, s, right, cy, font(SMALL + 2.0, REGULAR), theme.text2());
        }
        Value::Action => icon(
            c,
            Icon::ChevronRight,
            Point::new(right - 14.0, cy),
            32.0,
            if on { theme.text() } else { theme.text3() },
        ),
        Value::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(n: usize) -> Page {
        let mut rows = vec![Row::heading("Head")];
        rows.extend((0..n).map(|i| Row::new(format!("Row {i}"), Value::Action)));
        Page {
            title: "T".into(),
            sections: vec!["A".into()],
            icons: Vec::new(),
            section: 0,
            rows,
            first: 0,
            note: String::new(),
        }
    }

    #[test]
    fn the_focus_skips_headings_and_the_list_scrolls_to_it() {
        let p = page(20);
        assert_eq!(p.first_row(), 1);
        assert_eq!(p.next(1, false), 1, "nothing to stop on above");
        assert_eq!(p.next(1, true), 2);
        let first = p.first_for(20);
        assert!(first > 0);
        let p = Page { first, ..p };
        assert!(p.showing(20));
        // Back up to the top, the heading comes into sight with row 1.
        assert_eq!(p.first_for(1), 0);
    }
}
