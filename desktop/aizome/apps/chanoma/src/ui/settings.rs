//! The grown-ups' settings: sections down the left, the section's rows on
//! the right. Up and down move between rows; left and right change a
//! choice; the confirm button switches a switch or does an action; Back
//! goes to the sections, and from there out.
//!
//! The focus: `row` is the row (in the list) or the section (at the side),
//! `col` is 0 at the side and 1 in the list.

use aizome_kit::layout::{Point, Rect};
use aizome_kit::sidestep_ui::kurbo::{Circle, RoundedRect};
use aizome_kit::sidestep_ui::{Canvas, Color};
use aizome_kit::theme::Theme;

use super::look::*;
use super::{Fit, Focus, H, MARGIN, TOP, Target, W};

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
        let y = TOP + 20.0 + i as f64 * 84.0;
        Rect::new(MARGIN, y, MARGIN + SIDE_W, y + 72.0)
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
        // The title where Home has its clock.
        let head = Rect::new(0.0, 40.0, W, TOP + 10.0);
        if c.needs(head) {
            text(c, theme, &self.title, Point::new(MARGIN, 64.0), font(58.0, 800.0), INK);
            if !self.note.is_empty() {
                let w = measure(theme, &self.title, font(58.0, 800.0)).width;
                text(c, theme, &self.note, Point::new(MARGIN + w + 28.0, 84.0), font(30.0, 600.0), INK_SOFT);
            }
        }
        for (i, s) in self.sections.iter().enumerate() {
            let r = self.section_rect(i);
            if !c.needs(r.inflate(16.0, 16.0)) {
                continue;
            }
            let current = i == self.section;
            let on = focus.col == 0 && focus.row == i;
            if current || on {
                c.fill(&RoundedRect::from_rect(r, 20.0), if on { INK } else { Color::hex(0x2c3474) });
            }
            let ink = if on {
                Color::hex(0x141a3a)
            } else if current {
                INK
            } else {
                INK_SOFT
            };
            text(c, theme, s, Point::new(r.x0 + 28.0, r.y0 + 18.0), font(32.0, 700.0), ink);
            if on {
                focus_ring(c, r, 20.0);
            }
        }
        for i in (0..self.rows.len()).filter(|i| self.showing(*i)) {
            let r = self.row_rect(i);
            if !c.needs(r.inflate(16.0, 16.0)) {
                continue;
            }
            let row = &self.rows[i];
            if row.heading {
                text(c, theme, &row.label, Point::new(r.x0 + 8.0, r.y0 + 22.0), font(26.0, 800.0), FOCUS);
                continue;
            }
            let on = focus.col == 1 && focus.row == i;
            c.fill(&RoundedRect::from_rect(r, 20.0), if on { Color::hex(0x323b84) } else { Color::hex(0x232a5e) });
            let label_y = if row.detail.is_empty() { r.center().y - 20.0 } else { r.y0 + 10.0 };
            let value_w = value_width(theme, &row.value);
            let label = fit_text(theme, &row.label, r.width() - value_w - 90.0, font(32.0, 700.0));
            text(c, theme, &label, Point::new(r.x0 + 28.0, label_y), font(32.0, 700.0), INK);
            if !row.detail.is_empty() {
                let d = fit_text(theme, &row.detail, r.width() - value_w - 90.0, font(24.0, 600.0));
                text(c, theme, &d, Point::new(r.x0 + 28.0, r.y0 + 50.0), font(24.0, 600.0), INK_SOFT);
            }
            if let Some(b) = &row.badge {
                let lw = measure(theme, &label, font(32.0, 700.0)).width;
                let spec = font(20.0, 800.0);
                let bw = measure(theme, b, spec).width + 22.0;
                let br = Rect::new(r.x0 + 40.0 + lw, label_y + 4.0, r.x0 + 40.0 + lw + bw, label_y + 34.0);
                c.fill(&RoundedRect::from_rect(br, 15.0), FOCUS);
                text_centred(c, theme, b, br.center(), spec, Color::hex(0x2a1e05));
            }
            paint_value(c, theme, &row.value, r, on);
            if on {
                focus_ring(c, r, 20.0);
            }
        }
        // More below or above: a fade of the arrows' hint.
        if self.first > 0 {
            text_centred(
                c,
                theme,
                "▲",
                Point::new((LIST_X + W - MARGIN) / 2.0, LIST_TOP - 14.0),
                font(22.0, 700.0),
                INK_FAINT,
            );
        }
        if (0..self.rows.len()).any(|i| i >= self.first && !self.showing(i)) {
            text_centred(
                c,
                theme,
                "▼",
                Point::new((LIST_X + W - MARGIN) / 2.0, LIST_BOTTOM + 8.0),
                font(22.0, 700.0),
                INK_FAINT,
            );
        }
    }
}

fn value_width(theme: &Theme, v: &Value) -> f64 {
    match v {
        Value::Switch(_) => 110.0,
        Value::Choice(s) => measure(theme, s, font(30.0, 700.0)).width + 110.0,
        Value::Note(s) => measure(theme, s, font(28.0, 600.0)).width + 30.0,
        Value::Action => 50.0,
        Value::None => 0.0,
    }
}

fn paint_value(c: &mut Canvas, theme: &Theme, v: &Value, r: Rect, on: bool) {
    let right = r.x1 - 28.0;
    let cy = r.center().y;
    match v {
        Value::Switch(state) => {
            let track = Rect::new(right - 92.0, cy - 24.0, right, cy + 24.0);
            c.fill(&RoundedRect::from_rect(track, 24.0), if *state { GOOD } else { Color::hex(0x3a427a) });
            let knob = if *state { track.x1 - 24.0 } else { track.x0 + 24.0 };
            c.fill(&Circle::new(Point::new(knob, cy), 19.0), INK);
        }
        Value::Choice(s) => {
            let spec = font(30.0, 700.0);
            let w = measure(theme, s, spec).width;
            let arrows = if on { INK } else { INK_FAINT };
            text_right(c, theme, "›", right, cy - 22.0, font(40.0, 700.0), arrows);
            text(c, theme, s, Point::new(right - 40.0 - w, cy - 19.0), spec, INK);
            text(c, theme, "‹", Point::new(right - 72.0 - w, cy - 22.0), font(40.0, 700.0), arrows);
        }
        Value::Note(s) => text_right(c, theme, s, right, cy - 17.0, font(28.0, 600.0), INK_SOFT),
        Value::Action => {
            text_right(c, theme, "›", right, cy - 24.0, font(42.0, 700.0), if on { INK } else { INK_FAINT })
        }
        Value::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(n: usize) -> Page {
        let mut rows = vec![Row::heading("Head")];
        rows.extend((0..n).map(|i| Row::new(format!("Row {i}"), Value::Action)));
        Page { title: "T".into(), sections: vec!["A".into()], section: 0, rows, first: 0, note: String::new() }
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
