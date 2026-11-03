//! The tab bar's tabs. While they fit they are as wide as their titles, up to
//! 240pt. When they don't, the others shrink, the widest first, while the shown
//! tab keeps its width when space permits, down to a floor of a few words.
//! Past that the strip scrolls sideways under the wheel. It keeps the shown tab
//! in sight and counts the tabs out of sight at each end, amber when one rang.
//! A click on a count scrolls that way. The bar's buttons sit beside the
//! strip, so no number of tabs moves them. Tabs not shown also leave off a
//! part of the title most tabs share ("… | sekka"), since it doesn't tell
//! them apart.

use aizome_face::look;
use aizome_kit::Layoutable;
use aizome_kit::element::Element;
use aizome_kit::hash::PaintHasher;
use aizome_kit::layout::{Align, Constraints, Length, Point, Rect, Size};
use aizome_kit::sidestep_ui::PointerButton;
use aizome_kit::widget::{Event, EventCx, LayoutCx, PaintCx, Status, Widget};
use aizome_theme::typeface;

/// The widest a tab grows.
pub const WIDEST: f64 = 240.0;
/// The narrowest a tab shrinks before the strip scrolls.
const FLOOR: f64 = 96.0;
/// Between tabs, as between a toolbar's items.
const GAP: f64 = 4.0;
/// A count of the tabs out of sight at an end.
const COUNT: f64 = 34.0;
/// What separates the parts of a title: "Review PR #221 | sekka",
/// "callen@ganymede: ~/work".
const SEPARATORS: [&str; 5] = [" | ", " — ", " – ", " · ", ": "];

/// The tabs' labels: the shown tab's whole title, the others' without a
/// first or last part that at least half the titles (two or more) share.
pub fn labels(titles: &[&str], shown: usize) -> Vec<String> {
    let mut labels: Vec<String> = titles.iter().map(|title| (*title).to_owned()).collect();
    if titles.len() < 3 {
        return labels;
    }
    let tail = shared(titles.iter().map(|title| last_part(title)));
    let head = shared(titles.iter().map(|title| first_part(title)));
    for (index, label) in labels.iter_mut().enumerate() {
        if index == shown {
            continue;
        }
        if let Some(rest) = tail.and_then(|tail| label.strip_suffix(tail)).filter(|rest| !rest.trim().is_empty()) {
            *label = rest.to_owned();
        }
        if let Some(rest) = head.and_then(|head| label.strip_prefix(head)).filter(|rest| !rest.trim().is_empty()) {
            *label = rest.to_owned();
        }
    }
    labels
}

/// The last separator and what follows it: " | sekka".
fn last_part(title: &str) -> Option<&str> {
    SEPARATORS.iter().filter_map(|separator| title.rfind(separator)).max().map(|at| &title[at..])
}

/// What precedes the first separator, with it: "callen@ganymede: ".
fn first_part(title: &str) -> Option<&str> {
    SEPARATORS
        .iter()
        .filter_map(|separator| title.find(separator).map(|at| at + separator.len()))
        .min()
        .map(|end| &title[..end])
}

/// The part at least half of them have, when two or more do.
fn shared<'a>(parts: impl Iterator<Item = Option<&'a str>>) -> Option<&'a str> {
    let mut total = 0;
    let mut counts: Vec<(&str, usize)> = Vec::new();
    for part in parts {
        total += 1;
        let Some(part) = part else { continue };
        match counts.iter_mut().find(|(seen, _)| *seen == part) {
            Some((_, count)) => *count += 1,
            None => counts.push((part, 1)),
        }
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .filter(|(_, count)| *count >= 2 && count * 2 >= total)
        .map(|(part, _)| part)
}

/// The width the tabs other than the shown one share `room` at: the most
/// any is given, those narrower keeping their own width.
fn share(mut widths: Vec<f64>, room: f64) -> f64 {
    widths.sort_by(f64::total_cmp);
    let (mut room, mut left) = (room, widths.len());
    for width in widths {
        if width * left as f64 > room {
            return room / left as f64;
        }
        room -= width;
        left -= 1;
    }
    f64::INFINITY
}

/// `tabs` in a strip filling the room the bar leaves; `shown` is the tab
/// in front, and `bells` says which rang.
pub fn tab_strip<Msg>(tabs: Vec<Element<Msg>>, shown: usize, bells: Vec<bool>) -> Element<Msg> {
    let strip = TabStrip { shown, bells, before: Count::default(), after: Count::default(), width: 0.0 };
    Element::new(strip, tabs).width(Length::Fill)
}

struct TabStrip {
    shown: usize,
    bells: Vec<bool>,
    /// The tabs out of sight before and after the strip's ends, as last laid out.
    before: Count,
    after: Count,
    width: f64,
}

/// The tabs out of sight past one end of the strip.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Count {
    tabs: usize,
    /// One of them rang.
    rang: bool,
    /// Reserved counter width; never expands over the tabs between the ends.
    reach: f64,
}

/// Where the strip is scrolled to, kept between views.
#[derive(Default)]
struct Scrolled {
    offset: f64,
    content: f64,
    width: f64,
    /// The shown tab and how many there were when it was last brought into
    /// sight: changing either brings it into sight again.
    revealed: Option<(usize, usize)>,
    /// The shown tab is kept in sight as titles change, until the strip is
    /// scrolled by hand.
    following: bool,
}

impl TabStrip {
    fn scroll(cx: &mut EventCx, by: f64) -> Status {
        let st = cx.state::<Scrolled>();
        let before = st.offset;
        st.offset = (st.offset + by).clamp(0.0, (st.content - st.width).max(0.0));
        if st.offset == before {
            return Status::Ignored;
        }
        st.following = false;
        cx.relayout();
        cx.invalidate();
        Status::Handled
    }

    /// A count's rectangle, while it shows.
    fn count(&self, after: bool) -> Option<Rect> {
        match after {
            false => (self.before.tabs > 0).then(|| Rect::new(0.0, 0.0, self.before.reach, f64::INFINITY)),
            true => {
                (self.after.tabs > 0).then(|| Rect::new(self.width - self.after.reach, 0.0, self.width, f64::INFINITY))
            }
        }
    }
}

impl Widget for TabStrip {
    fn name(&self) -> &'static str {
        "tab-strip"
    }

    fn layout(&mut self, cx: &mut LayoutCx, bc: Constraints) -> Size {
        let n = cx.child_count();
        let tall = bc.max.height;
        let natural: Vec<Size> =
            (0..n).map(|i| cx.layout_child(i, Constraints::loose(Size::new(WIDEST, tall)))).collect();
        let gaps = GAP * n.saturating_sub(1) as f64;
        let shown = self.shown.min(n.saturating_sub(1));
        let mut widths: Vec<f64> = natural.iter().map(|size| size.width).collect();
        let view = if bc.max.width.is_finite() { bc.max.width } else { widths.iter().sum::<f64>() + gaps };
        // Even below the normal window minimum, leave tab content between
        // the two counters rather than letting their hit regions overlap.
        let counter = COUNT.min(view / 3.0);
        if widths.iter().sum::<f64>() + gaps > view {
            if n > 1 {
                let others = widths.iter().enumerate().filter(|(i, _)| *i != shown).map(|(_, width)| *width).collect();
                let most = share(others, view - widths[shown] - gaps).max(FLOOR);
                for (i, width) in widths.iter_mut().enumerate() {
                    if i != shown {
                        *width = width.min(most.floor());
                    }
                }
            }
            // A long active tab must fit between the counters. A lone tab
            // can use the whole strip and never needs a hidden-tab counter.
            if let Some(width) = widths.get_mut(shown) {
                let room = if n > 1 { view - 2.0 * counter } else { view };
                *width = width.min(room);
            }
            for (i, width) in widths.iter().enumerate() {
                if *width != natural[i].width {
                    cx.layout_child(i, Constraints::tight(Size::new(*width, natural[i].height)));
                }
            }
        }
        let height = natural.iter().map(|size| size.height).fold(bc.min.height, f64::max).min(tall);
        let mut starts = Vec::with_capacity(n);
        let mut x = 0.0;
        for width in &widths {
            starts.push(x);
            x += width + GAP;
        }
        let content = (x - GAP).max(0.0);
        let most = (content - view).max(0.0);
        let st = cx.state::<Scrolled>();
        if st.revealed != Some((shown, n)) || st.width != view {
            st.revealed = Some((shown, n));
            st.following = true;
        }
        if st.following
            && let Some(&start) = starts.get(shown)
        {
            // Clear of the counts at the ends.
            let margin = if most > 0.0 { counter } else { 0.0 };
            let end = start + widths[shown];
            if start - margin < st.offset {
                st.offset = start - margin;
            }
            if end + margin > st.offset + view {
                st.offset = end + margin - view;
            }
        }
        st.offset = st.offset.clamp(0.0, most);
        st.content = content;
        st.width = view;
        let offset = st.offset;
        // Count tabs partly or wholly hidden at each end, without covering
        // their remaining visible portions between the fixed-width counters.
        let mut before = Count::default();
        if offset > 0.5 {
            let first = starts.iter().position(|start| start - offset >= counter).unwrap_or(n);
            before = Count { tabs: first, rang: self.bells.iter().take(first).any(|rang| *rang), reach: counter };
        }
        let mut after = Count::default();
        if offset < most - 0.5 {
            let last = (0..n).rev().find(|&i| starts[i] + widths[i] - offset <= view - counter);
            let from = last.map_or(0, |i| i + 1);
            after = Count { tabs: n - from, rang: self.bells.iter().skip(from).any(|rang| *rang), reach: counter };
        }
        if (before, after, view) != (self.before, self.after, self.width) {
            cx.invalidate_rect(Rect::new(0.0, 0.0, view, height));
        }
        (self.before, self.after, self.width) = (before, after, view);
        for (i, start) in starts.iter().enumerate() {
            let tab_height = cx.child_size(i).height;
            cx.place_child(i, Point::new(start - offset, ((height - tab_height) / 2.0).round()));
        }
        bc.constrain(Size::new(view, height))
    }

    fn paint_over(&self, cx: &mut PaintCx) {
        let t = cx.theme();
        let b = cx.bounds();
        for (after, count) in [(false, self.before), (true, self.after)] {
            let Some(r) = self.count(after) else { continue };
            let r = Rect::new(r.x0, b.y0, r.x1, b.y1);
            cx.canvas.fill_rect(r, t.chrome());
            let edge = if after { r.x0 } else { r.x1 - 1.0 };
            cx.canvas.fill_rect(Rect::new(edge, b.y0 + 5.0, edge + 1.0, b.y1 - 5.0), t.line());
            let color = if count.rang { look::Hue::Amber.color(t) } else { t.text2() };
            let spec = look::mono(11.0, typeface::WEIGHT_MEDIUM, color);
            let label = if after { format!("{}›", count.tabs) } else { format!("‹{}", count.tabs) };
            cx.draw_text_in(&label, &spec, r, Align::Center);
        }
    }

    fn hash(&self, h: &mut PaintHasher) {
        for count in [self.before, self.after] {
            h.add(&(count.tabs, count.rang, count.reach.to_bits()));
        }
        h.add(&self.width.to_bits());
    }

    fn hit_children(&self, pos: Point) -> bool {
        ![false, true].into_iter().filter_map(|after| self.count(after)).any(|r| r.contains(pos))
    }

    fn solid(&self) -> bool {
        true
    }

    fn clips(&self) -> bool {
        true
    }

    fn event(&mut self, cx: &mut EventCx, event: &Event) -> Status {
        match event {
            Event::Scroll { delta, .. } => {
                Self::scroll(cx, if delta.x.abs() > delta.y.abs() { delta.x } else { delta.y })
            }
            Event::PointerDown { pos, button: PointerButton::Left, .. } => {
                // A count scrolls most of the strip's width that way.
                let page = (self.width - 2.0 * COUNT).max(COUNT);
                match [false, true].into_iter().find(|after| self.count(*after).is_some_and(|r| r.contains(*pos))) {
                    Some(after) => Self::scroll(cx, if after { page } else { -page }),
                    None => Status::Ignored,
                }
            }
            _ => Status::Ignored,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_not_shown_leave_off_what_most_titles_share() {
        let titles = [
            "Verify nightly build status | sekka",
            "✳ Sekka PR 196 multi-window design",
            "Review PR #221 | sekka",
            "Check Uta audio quality | uta",
        ];
        let shown = labels(&titles, 2);
        assert_eq!(shown[0], "Verify nightly build status");
        assert_eq!(shown[1], titles[1]);
        assert_eq!(shown[2], titles[2], "the shown tab keeps its whole title");
        assert_eq!(shown[3], titles[3], "a part only it has tells it apart");
        let shells = ["callen@ganymede: ~/work/sekka", "callen@ganymede: ~", "callen@ganymede: /etc"];
        assert_eq!(labels(&shells, 0), ["callen@ganymede: ~/work/sekka", "~", "/etc"]);
        // Two tabs, or a part fewer than half share, keep their titles.
        assert_eq!(labels(&titles[..2], 1), titles[..2]);
        let few = ["a | x", "b | x", "c | y", "d | z", "e"];
        assert_eq!(labels(&few, 4), few);
        // Nothing is left off that would leave nothing.
        assert_eq!(labels(&[" | sekka", "a | sekka", "b | sekka"], 1), [" | sekka", "a | sekka", "b"]);
    }

    #[test]
    fn the_others_share_the_room_the_shown_tab_leaves() {
        assert_eq!(share(vec![50.0, 100.0, 200.0], 400.0), f64::INFINITY);
        assert_eq!(share(vec![50.0, 200.0, 200.0], 300.0), 125.0);
        assert_eq!(share(vec![200.0, 200.0], 100.0), 50.0);
        assert_eq!(share(Vec::new(), 10.0), f64::INFINITY);
    }
}
