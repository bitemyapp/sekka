//! Selecting text: where a point falls between characters, what a
//! selection covers on each page, the rectangles that show it, and the
//! text it copies. Characters come from the worker in reading order with
//! their boxes (`protocol::PageText`); those it made up (the spaces
//! between words, line ends) have no box.

use crate::protocol::{PageRect, PageText};

/// A place between two characters: page `page`, before character `index`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Caret {
    pub page: u32,
    pub index: usize,
}

/// Text chosen by dragging from `anchor` to `focus`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Selection {
    pub anchor: Caret,
    pub focus: Caret,
}

impl Selection {
    pub fn ordered(&self) -> (Caret, Caret) {
        if self.anchor <= self.focus { (self.anchor, self.focus) } else { (self.focus, self.anchor) }
    }

    pub fn is_empty(&self) -> bool {
        self.anchor == self.focus
    }

    /// The characters it covers on `page`, which has `len`.
    pub fn on_page(&self, page: u32, len: usize) -> Option<(usize, usize)> {
        let (a, b) = self.ordered();
        if page < a.page || page > b.page {
            return None;
        }
        let s = if page == a.page { a.index.min(len) } else { 0 };
        let e = if page == b.page { b.index.min(len) } else { len };
        (s < e).then_some((s, e))
    }
}

fn has_box(b: &PageRect) -> bool {
    b[2] > b[0] && b[3] > b[1]
}

fn mid_x(b: &PageRect) -> f64 {
    (f64::from(b[0]) + f64::from(b[2])) / 2.0
}

/// Whether two boxes are on one line: they overlap by half the shorter's
/// height.
fn same_line(a: &PageRect, b: &PageRect) -> bool {
    let overlap = f64::from(a[3].min(b[3]) - a[1].max(b[1]));
    let shorter = f64::from((a[3] - a[1]).min(b[3] - b[1]));
    overlap >= shorter * 0.5
}

/// The place nearest `at` (page points) between characters: before the
/// character under it or after it, by which half; off the text, by the
/// nearest character on the nearest line.
pub fn caret_at(text: &PageText, at: (f64, f64)) -> Option<usize> {
    let (x, y) = at;
    let mut best: Option<(f64, usize)> = None;
    for (i, b) in text.boxes.iter().enumerate() {
        if !has_box(b) {
            continue;
        }
        let (x0, y0, x1, y1) = (f64::from(b[0]), f64::from(b[1]), f64::from(b[2]), f64::from(b[3]));
        let dx = if x < x0 {
            x0 - x
        } else if x > x1 {
            x - x1
        } else {
            0.0
        };
        let dy = if y < y0 {
            y0 - y
        } else if y > y1 {
            y - y1
        } else {
            0.0
        };
        // Lines are what matters: across one is far.
        let d = dy * 1000.0 + dx;
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    let (_, i) = best?;
    Some(if x < mid_x(&text.boxes[i]) { i } else { i + 1 })
}

/// The index of the character under `at`, if one is.
pub fn char_at(text: &PageText, at: (f64, f64)) -> Option<usize> {
    text.boxes.iter().position(|b| {
        has_box(b)
            && at.0 >= f64::from(b[0])
            && at.0 <= f64::from(b[2])
            && at.1 >= f64::from(b[1])
            && at.1 <= f64::from(b[3])
    })
}

/// The word around character `i`: letters and digits (and the marks
/// between them), else the character alone.
pub fn word(text: &PageText, i: usize) -> (usize, usize) {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '\'' || c == '’';
    if i >= text.chars.len() || !is_word(text.chars[i]) {
        return (i, (i + 1).min(text.chars.len()));
    }
    let mut s = i;
    while s > 0 && is_word(text.chars[s - 1]) {
        s -= 1;
    }
    let mut e = i + 1;
    while e < text.chars.len() && is_word(text.chars[e]) {
        e += 1;
    }
    (s, e)
}

/// The line around character `i`: from the line end before it to the
/// next one.
pub fn line(text: &PageText, i: usize) -> (usize, usize) {
    let n = text.chars.len();
    let i = i.min(n);
    let s = text.chars[..i].iter().rposition(|c| *c == '\n').map_or(0, |p| p + 1);
    let e = text.chars[i..].iter().position(|c| *c == '\n').map_or(n, |p| i + p);
    (s, e)
}

/// The rectangles that show characters `s..e`: their boxes joined along
/// each line.
pub fn rects(text: &PageText, s: usize, e: usize) -> Vec<PageRect> {
    let mut out: Vec<PageRect> = Vec::new();
    for b in text.boxes.get(s..e.min(text.boxes.len())).unwrap_or(&[]) {
        if !has_box(b) {
            continue;
        }
        match out.last_mut() {
            Some(r) if same_line(r, b) && b[0] >= r[0] - 1.0 => {
                r[0] = r[0].min(b[0]);
                r[1] = r[1].min(b[1]);
                r[2] = r[2].max(b[2]);
                r[3] = r[3].max(b[3]);
            }
            _ => out.push(*b),
        }
    }
    out
}

/// The text of characters `s..e`, line ends kept.
pub fn extract(text: &PageText, s: usize, e: usize) -> String {
    text.chars.get(s..e.min(text.chars.len())).unwrap_or(&[]).iter().filter(|c| **c != '\0').collect()
}

/// What `sel` copies of page `page`, whose text is `t` (`None` if it
/// covers none of it).
pub fn page_part(sel: &Selection, page: u32, t: &PageText) -> Option<String> {
    let (s, e) = sel.on_page(page, t.chars.len())?;
    Some(extract(t, s, e).trim_end_matches('\n').to_owned())
}

/// What `sel` copies from the pages `texts` gives (in page order; pages
/// whose text hasn't come are left out, so the program asks for them
/// before it copies: `app::Copying`).
pub fn copy<'a>(sel: &Selection, texts: impl Fn(u32) -> Option<&'a PageText>) -> String {
    let (a, b) = sel.ordered();
    let parts: Vec<String> = (a.page..=b.page).filter_map(|p| page_part(sel, p, texts(p)?)).collect();
    parts.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "ab cd" on one line (each letter 10 wide from x 0, y 0–10; the
    /// space made up), a line end, then "ef" below.
    fn sample() -> PageText {
        let b = |x: f32, y: f32| [x, y, x + 10.0, y + 10.0];
        PageText {
            chars: vec!['a', 'b', ' ', 'c', 'd', '\n', 'e', 'f'],
            boxes: vec![
                b(0.0, 0.0),
                b(10.0, 0.0),
                [0.0; 4],
                b(30.0, 0.0),
                b(40.0, 0.0),
                [0.0; 4],
                b(0.0, 20.0),
                b(10.0, 20.0),
            ],
        }
    }

    #[test]
    fn points_fall_between_characters() {
        let t = sample();
        assert_eq!(caret_at(&t, (2.0, 5.0)), Some(0));
        assert_eq!(caret_at(&t, (8.0, 5.0)), Some(1));
        // Past the end of the line: after its last character.
        assert_eq!(caret_at(&t, (90.0, 5.0)), Some(5));
        // Below everything: on the last line.
        assert_eq!(caret_at(&t, (11.0, 80.0)), Some(7));
        assert_eq!(caret_at(&PageText::default(), (0.0, 0.0)), None);
        assert_eq!(char_at(&t, (33.0, 3.0)), Some(3));
        assert_eq!(char_at(&t, (25.0, 3.0)), None);
    }

    #[test]
    fn words_and_lines() {
        let t = sample();
        assert_eq!(word(&t, 4), (3, 5));
        assert_eq!(word(&t, 2), (2, 3));
        assert_eq!(line(&t, 1), (0, 5));
        assert_eq!(line(&t, 7), (6, 8));
    }

    #[test]
    fn rectangles_join_along_a_line() {
        let t = sample();
        assert_eq!(rects(&t, 0, 8), vec![[0.0, 0.0, 50.0, 10.0], [0.0, 20.0, 20.0, 30.0]]);
        assert_eq!(rects(&t, 3, 4), vec![[30.0, 0.0, 40.0, 10.0]]);
    }

    #[test]
    fn copies_across_pages() {
        let t = sample();
        let sel = Selection { anchor: Caret { page: 2, index: 1 }, focus: Caret { page: 0, index: 3 } };
        assert_eq!(sel.on_page(0, 8), Some((3, 8)));
        assert_eq!(sel.on_page(1, 8), Some((0, 8)));
        assert_eq!(sel.on_page(2, 8), Some((0, 1)));
        assert_eq!(sel.on_page(3, 8), None);
        let copied = copy(&sel, |p| (p != 1).then_some(&t));
        assert_eq!(copied, "cd\nef\na");
        let one = Selection { anchor: Caret { page: 0, index: 0 }, focus: Caret { page: 0, index: 5 } };
        assert_eq!(copy(&one, |_| Some(&t)), "ab cd");
    }
}
